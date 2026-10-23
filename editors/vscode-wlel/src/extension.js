// Wlel language client — zero dependencies.
//
// `wlel lsp` speaks LSP 3.17 over stdio, so the client is a small
// JSON-RPC-over-pipes bridge: spawn the server, forward Content-Length
// framed messages in both directions, and let VS Code's own language client
// features (diagnostics, hover, definition, completion) do the rest.
//
// Plain JavaScript on purpose: an extension that needs `tsc` and a
// node_modules tree before it can run is a worse trade than 200 readable
// lines, and this file has no build step.
'use strict';

const vscode = require('vscode');
const cp = require('child_process');

/** @type {cp.ChildProcessWithoutNullStreams | undefined} */
let server;
/** @type {Buffer} bytes buffered from the server's stdout */
let pending = Buffer.alloc(0);
let nextId = 1;
/** id -> resolve/reject for requests we sent */
const inFlight = new Map();
/** @type {vscode.DiagnosticCollection} */
let diagnostics;
let output;
let workspaceRoot;
let restartTimer;
/** bumped on every start/stop: a stale process must not restart itself */
let generation = 0;
/** true between `stop`/`deactivate` and the next `activate` */
let stopping = false;
/** the server is ready to receive document traffic (initialize completed) */
let ready = false;
/** notifications produced before the handshake finished */
const queued = [];

function config() {
  const c = vscode.workspace.getConfiguration('wlel');
  return {
    path: c.get('serverPath', 'wlel'),
    args: c.get('serverArgs', ['lsp']),
    trace: c.get('trace.server', 'off'),
  };
}

function log(msg) {
  if (output) output.appendLine(`[wlel] ${msg}`);
}

function rawSend(payload) {
  if (!server || server.killed) return;
  const body = Buffer.from(JSON.stringify(payload), 'utf8');
  server.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
  server.stdin.write(body);
}

/**
 * Queue notifications until `initialize` has been answered: a `didOpen`
 * arriving before the handshake would be dropped by a strict server, and VS
 * Code opens the document before our first round-trip completes.
 */
function send(payload) {
  if (!ready && payload.method !== 'initialize') {
    queued.push(payload);
    return;
  }
  rawSend(payload);
}

/** Send a request and resolve with its result (used for initialize). */
function request(method, params) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    inFlight.set(id, { resolve, reject });
    send({ jsonrpc: '2.0', id, method, params });
  });
}

function notify(method, params) {
  send({ jsonrpc: '2.0', method, params });
}

/** Feed stdout bytes through the Content-Length framing. */
function onData(chunk) {
  pending = Buffer.concat([pending, chunk]);
  for (;;) {
    const headerEnd = pending.indexOf('\r\n\r\n');
    if (headerEnd < 0) return;
    const header = pending.slice(0, headerEnd).toString('ascii');
    const match = /Content-Length:\s*(\d+)/i.exec(header);
    if (!match) {
      // unparseable header: drop it and keep going
      pending = pending.slice(headerEnd + 4);
      continue;
    }
    const length = Number(match[1]);
    const start = headerEnd + 4;
    if (pending.length < start + length) return;
    const body = pending.slice(start, start + length).toString('utf8');
    pending = pending.slice(start + length);
    let msg;
    try {
      msg = JSON.parse(body);
    } catch (e) {
      log(`malformed message from server: ${e.message}`);
      continue;
    }
    handle(msg);
  }
}

function handle(msg) {
  const { trace } = config();
  if (trace !== 'off') log(`<- ${JSON.stringify(msg).slice(0, 400)}`);

  if (msg.id !== undefined && (msg.result !== undefined || msg.error !== undefined)) {
    const waiter = inFlight.get(msg.id);
    if (waiter) {
      inFlight.delete(msg.id);
      if (msg.error) waiter.reject(new Error(msg.error.message || 'server error'));
      else waiter.resolve(msg.result);
    }
    return;
  }
  switch (msg.method) {
    case 'textDocument/publishDiagnostics': {
      const { uri, diagnostics: diags } = msg.params;
      const converted = (diags || []).map((d) => {
        const range = new vscode.Range(
          d.range.start.line,
          d.range.start.character,
          d.range.end.line,
          d.range.end.character
        );
        const out = new vscode.Diagnostic(
          range,
          d.message,
          d.severity === 1 ? vscode.DiagnosticSeverity.Error : vscode.DiagnosticSeverity.Warning
        );
        out.source = 'wlel';
        return out;
      });
      diagnostics.set(vscode.Uri.parse(uri), converted);
      return;
    }
    case 'window/logMessage':
      log(msg.params && msg.params.message ? msg.params.message : '');
      return;
    // the server does not send requests of its own; answer politely anyway
    case 'workspace/configuration':
      send({ jsonrpc: '2.0', id: msg.id, result: [] });
      return;
    default:
      if (msg.id !== undefined) {
        send({ jsonrpc: '2.0', id: msg.id, result: null });
      }
  }
}

function uriOf(doc) {
  return doc.uri.toString();
}

function startServer(context) {
  const { path, args } = config();
  const mine = ++generation;
  stopping = false;
  log(`starting: ${path} ${args.join(' ')}`);

  server = cp.spawn(path, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  server.stdout.on('data', onData);
  server.stderr.on('data', (d) => log(d.toString().trimEnd()));
  server.on('error', (err) => {
    log(`cannot start '${path}': ${err.message}`);
    vscode.window.showErrorMessage(
      `Wlel: cannot run '${path}'. Install the compiler or set "wlel.serverPath".`
    );
  });
  server.on('exit', (code, signal) => {
    log(`server exited (code ${code}, signal ${signal})`);
    // a restart or a stop already replaced this process: its exit is expected
    if (mine !== generation || stopping) return;
    ready = false;
    for (const [, waiter] of inFlight) waiter.reject(new Error('server exited'));
    inFlight.clear();
    // a crash should not leave the editor without a language server: restart
    // after a pause so a broken binary does not spin
    clearTimeout(restartTimer);
    restartTimer = setTimeout(() => {
      if (mine === generation && !stopping) startServer(context);
    }, 3000);
  });

  const rootUri = workspaceRoot ? vscode.Uri.file(workspaceRoot).toString() : null;
  request('initialize', {
    processId: process.pid,
    rootUri,
    capabilities: {
      textDocument: {
        synchronization: { dynamicRegistration: false },
        hover: { contentFormat: ['markdown', 'plaintext'] },
        definition: {},
        completion: { completionItem: { snippetSupport: false } },
        publishDiagnostics: { relatedInformation: false },
      },
      workspace: { workspaceFolders: false },
    },
  })
    .then(() => {
      rawSend({ jsonrpc: '2.0', method: 'initialized', params: {} });
      ready = true;
      for (const msg of queued.splice(0)) rawSend(msg);
      // replay every open document: the server started after they were opened
      for (const doc of vscode.workspace.textDocuments) {
        if (doc.languageId === 'wlel') open(doc);
      }
    })
    .catch((e) => log(`initialize failed: ${e.message}`));
}

function open(doc) {
  notify('textDocument/didOpen', {
    textDocument: {
      uri: uriOf(doc),
      languageId: 'wlel',
      version: doc.version,
      text: doc.getText(),
    },
  });
}

function change(doc) {
  // full-document sync: the compiler re-parses the whole buffer anyway, and
  // incremental ranges would have to agree with UTF-16 offsets exactly
  notify('textDocument/didChange', {
    textDocument: { uri: uriOf(doc), version: doc.version },
    contentChanges: [{ text: doc.getText() }],
  });
}

function close(doc) {
  notify('textDocument/didClose', { textDocument: { uri: uriOf(doc) } });
  diagnostics.delete(doc.uri);
}

/** Convert a VS Code position to the LSP shape. */
function lspPosition(pos) {
  return { line: pos.line, character: pos.character };
}

/** Convert an LSP range/position pair to a VS Code Range. */
function toRange(r) {
  if (!r) return new vscode.Range(0, 0, 0, 0);
  return new vscode.Range(r.start.line, r.start.character, r.end.line, r.end.character);
}

function activate(context) {
  diagnostics = vscode.languages.createDiagnosticCollection('wlel');
  output = vscode.window.createOutputChannel('Wlel');
  context.subscriptions.push(diagnostics, output);

  const folder = vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0];
  workspaceRoot = folder ? folder.uri.fsPath : undefined;

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (doc.languageId === 'wlel') open(doc);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.languageId === 'wlel') change(e.document);
    }),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      if (doc.languageId === 'wlel') close(doc);
    }),

    // hover: the server answers with markdown (a code block + a note)
    vscode.languages.registerHoverProvider('wlel', {
      provideHover: async (doc, pos) => {
        const res = await request('textDocument/hover', {
          textDocument: { uri: uriOf(doc) },
          position: lspPosition(pos),
        }).catch(() => null);
        if (!res || !res.contents) return null;
        const value =
          typeof res.contents === 'string'
            ? res.contents
            : res.contents.value || res.contents;
        const md = new vscode.MarkdownString(typeof value === 'string' ? value : '');
        return new vscode.Hover(md, res.range ? toRange(res.range) : undefined);
      },
    }),

    // go-to-definition: the server returns a single Location
    vscode.languages.registerDefinitionProvider('wlel', {
      provideDefinition: async (doc, pos) => {
        const res = await request('textDocument/definition', {
          textDocument: { uri: uriOf(doc) },
          position: lspPosition(pos),
        }).catch(() => null);
        if (!res) return null;
        const locs = Array.isArray(res) ? res : [res];
        return locs
          .filter((l) => l && l.uri)
          .map((l) => new vscode.Location(vscode.Uri.parse(l.uri), toRange(l.range)));
      },
    }),

    // completion: the server returns a flat item list
    vscode.languages.registerCompletionItemProvider('wlel', {
      provideCompletionItems: async (doc, pos) => {
        const res = await request('textDocument/completion', {
          textDocument: { uri: uriOf(doc) },
          position: lspPosition(pos),
        }).catch(() => null);
        if (!res) return undefined;
        const items = Array.isArray(res) ? res : res.items || [];
        const word = doc.getWordRangeAtPosition(pos);
        return items.map((i) => {
          const kind = mapKind(i.kind);
          const item = new vscode.CompletionItem(i.label, kind);
          if (i.detail) item.detail = i.detail;
          if (i.documentation) {
            item.documentation =
              typeof i.documentation === 'string' ? i.documentation : i.documentation.value;
          }
          if (word) item.range = word;
          return item;
        });
      },
    }),

    // a config change means a different binary: restart cleanly
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration('wlel')) restart(context);
    }),
    { dispose: () => stop() },
  );

  startServer(context);
}

function mapKind(kind) {
  // LSP CompletionItemKind -> VS Code CompletionItemKind
  const K = vscode.CompletionItemKind;
  switch (kind) {
    case 2:
    case 3:
      return K.Function;
    case 7:
      return K.Class;
    case 8:
      return K.Interface;
    case 9:
      return K.Module;
    case 13:
      return K.Enum;
    case 14:
      return K.Keyword;
    case 20:
      return K.EnumMember;
    case 21:
      return K.Constant;
    case 22:
      return K.Struct;
    case 6:
      return K.Variable;
    default:
      return K.Text;
  }
}

function stop() {
  stopping = true;
  generation++;
  ready = false;
  queued.length = 0;
  clearTimeout(restartTimer);
  for (const [, waiter] of inFlight) waiter.reject(new Error('extension stopped'));
  inFlight.clear();
  if (server && !server.killed) {
    notify('shutdown');
    notify('exit');
    server.stdin.end();
    const dying = server;
    server = undefined;
    setTimeout(() => dying.kill(), 500);
  }
}

function restart(context) {
  // a config change means a different binary: drop the process and start over
  stopping = true;
  generation++;
  ready = false;
  pending = Buffer.alloc(0);
  queued.length = 0;
  clearTimeout(restartTimer);
  for (const [, waiter] of inFlight) waiter.reject(new Error('server restarting'));
  inFlight.clear();
  if (server && !server.killed) {
    const dying = server;
    server = undefined;
    dying.kill();
  }
  startServer(context);
}

function deactivate() {
  stopping = true;
  generation++;
  clearTimeout(restartTimer);
  if (server && !server.killed) {
    const dying = server;
    server = undefined;
    dying.stdin.end();
    dying.kill();
  }
}

module.exports = { activate, deactivate };
