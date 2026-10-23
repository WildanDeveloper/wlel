// Exercises the extension the way VS Code drives it: load src/extension.js
// against a stub `vscode` module, start the real `wlel lsp`, and call the
// providers that activate() registered.
//
//   WLEL_BIN=/path/to/wlel node test/bridge.js
//
// Exits non-zero on the first unmet expectation, so `cargo test` can run it.

'use strict';

const assert = require('assert');
const Module = require('module');
const os = require('os');
const path = require('path');

const binary = process.env.WLEL_BIN;
if (!binary) {
  console.error('WLEL_BIN is not set');
  process.exit(2);
}

// ---- a stub of the parts of the VS Code API the extension uses ------------

const log = [];
const published = [];
let openHandler;
let providers = {};

class Position {
  constructor(line, character) {
    this.line = line;
    this.character = character;
  }
}
class Range {
  constructor(a, b, c, d) {
    this.start = { line: a, character: b };
    this.end = { line: c, character: d };
  }
}
class Diagnostic {
  constructor(range, message, severity) {
    this.range = range;
    this.message = message;
    this.severity = severity;
    this.source = undefined;
  }
}
class MarkdownString {
  constructor(value) {
    this.value = value;
  }
}
class Hover {
  constructor(contents, range) {
    this.contents = contents;
    this.range = range;
  }
}
class Location {
  constructor(uri, range) {
    this.uri = uri;
    this.range = range;
  }
}
class CompletionItem {
  constructor(label, kind) {
    this.label = label;
    this.kind = kind;
  }
}

const uri = (s) => ({ toString: () => s, fsPath: s });
const vscode = {
  workspace: {
    getConfiguration: () => ({
      get: (key, fallback) =>
        key === 'serverPath' ? binary : key === 'serverArgs' ? ['lsp'] : fallback,
    }),
    workspaceFolders: undefined,
    textDocuments: [],
    onDidOpenTextDocument: (cb) => ((openHandler = cb), { dispose() {} }),
    onDidChangeTextDocument: () => ({ dispose() {} }),
    onDidCloseTextDocument: () => ({ dispose() {} }),
    onDidChangeConfiguration: () => ({ dispose() {} }),
  },
  languages: {
    createDiagnosticCollection: () => ({
      set: (u, diags) => published.push({ uri: u.toString(), diagnostics: diags }),
      delete: () => {},
      dispose() {},
    }),
    registerHoverProvider: (id, p) => ((providers.hover = p), { dispose() {} }),
    registerDefinitionProvider: (id, p) => ((providers.definition = p), { dispose() {} }),
    registerCompletionItemProvider: (id, p) => ((providers.completion = p), { dispose() {} }),
  },
  window: {
    createOutputChannel: () => ({ appendLine: (l) => log.push(l), dispose() {} }),
    showErrorMessage: (m) => log.push('error: ' + m),
  },
  DiagnosticSeverity: { Error: 0, Warning: 1 },
  Diagnostic,
  Range,
  Position,
  Hover,
  MarkdownString,
  Location,
  CompletionItem,
  CompletionItemKind: {
    Function: 2,
    Class: 7,
    Interface: 8,
    Module: 9,
    Enum: 13,
    Keyword: 14,
    EnumMember: 20,
    Constant: 21,
    Struct: 22,
    Variable: 6,
    Text: 1,
  },
  Uri: {
    parse: (s) => uri(s),
    file: (s) => uri('file://' + s),
  },
};

// intercept require('vscode')
const original = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  if (request === 'vscode') return 'vscode';
  return original.call(this, request, ...rest);
};
require.cache.vscode = { id: 'vscode', filename: 'vscode', loaded: true, exports: vscode };

// ---- drive the extension --------------------------------------------------

const root = path.resolve(__dirname, '..');
const extension = require(path.join(root, 'src/extension.js'));

const context = {
  subscriptions: [],
  globalState: { get: () => false, update() {} },
};
extension.activate(context);

const file = path.join(os.tmpdir(), `wlel-bridge-${process.pid}.wlel`);
const good = [
  'use std;',
  'fn main() -> int {',
  '    x := 1;',
  '    std::println_int(x);',
  '    return 0;',
  '}',
  '',
].join('\n');

function document(text, version) {
  return {
    uri: uri('file://' + file),
    languageId: 'wlel',
    version,
    getText: () => text,
    getWordRangeAtPosition: () => new Range(3, 4, 3, 5),
  };
}

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

(async () => {
  // VS Code opens the document right after activation; the extension has to
  // queue this until `initialize` has been answered
  const doc = document(good, 1);
  openHandler(doc);

  const hover = await providers.hover.provideHover(doc, new Position(2, 5));
  assert.ok(hover, 'hover returned nothing');
  assert.match(hover.contents.value, /x: int/, 'hover should describe x as int');

  const definition = await providers.definition.provideDefinition(doc, new Position(3, 21));
  assert.ok(definition && definition.length >= 1, 'definition returned nothing');
  assert.strictEqual(definition[0].range.start.line, 2, 'definition should land on the x declaration');
  assert.strictEqual(definition[0].range.start.character, 4);

  const completion = await providers.completion.provideCompletionItems(doc, new Position(3, 4));
  assert.ok(completion && completion.length > 20, 'completion should list locals, std and keywords');
  assert.ok(
    completion.some((item) => item.label === 'x'),
    'completion should contain the local x'
  );

  // A buffer with a type error must produce a diagnostic. The extension
  // syncs full documents on every change; re-opening the same URI with new
  // text makes the server re-analyse it, which is the shortest path from
  // this harness to a `publishDiagnostics` notification.
  const broken = ['fn main() -> int {', '    x := "s";', '    return x;', '}', ''].join('\n');
  const last = published.length;
  openHandler(document(broken, 2));
  await wait(400);
  assert.ok(published.length > last, 'no diagnostics were published for the broken buffer');
  const messages = published
    .slice(last)
    .flatMap((entry) => entry.diagnostics.map((d) => d.message));
  assert.ok(
    messages.some((m) => /return type mismatch/.test(m)),
    `expected a return type mismatch, got: ${JSON.stringify(messages)}`
  );

  console.log('bridge ok: hover, definition, completion, diagnostics');
  extension.deactivate();
  await wait(100);
  process.exit(0);
})().catch((err) => {
  console.error(err && err.stack ? err.stack : err);
  console.error('extension log:\n' + log.join('\n'));
  extension.deactivate();
  process.exit(1);
});
