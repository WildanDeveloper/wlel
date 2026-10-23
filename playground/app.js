// Wlel playground: load the WASM front-end and drive it from a textarea.
//
// The module's ABI is in playground/wasm/src/lib.rs:
//   wlel_alloc(len) -> ptr
//   wlel_analyze(ptr, len) -> result length
//   wlel_result_ptr() -> ptr
//   wlel_dealloc(ptr, len)
// The result is a JSON document. There is no wasm-bindgen.

'use strict';

const statusEl = document.getElementById('status');
const sourceEl = document.getElementById('source');
const diagnosticsEl = document.getElementById('diagnostics');
const formattedEl = document.getElementById('formatted');
const formattedWrap = document.getElementById('formatted-wrap');
const formatButton = document.getElementById('format');
const exampleButtons = document.getElementById('examples');

const EXAMPLES = {
  hello: `use std;

fn main() -> int {
    name := "world";
    std::println_str("hello, " + name);
    return 0;
}
`,
  enums: `use std;

enum Shape {
    Circle(float),
    Rect(float, float),
    Point,
}

fn area(s: Shape) -> float {
    return match s {
        Circle(r) => 3.141592653589793 * r * r,
        Rect(w, h) => w * h,
        Point => 0.0,
    };
}

fn main() -> int {
    std::println_float(area(Circle(1.0)));
    std::println_float(area(Rect(2.0, 3.0)));
    return 0;
}
`,
  generics: `use std;

struct Box[T] {
    value: T,
}

fn unwrap[T](b: Box[T]) -> T {
    return b.value;
}

fn main() -> int {
    n := Box { value: 41 };
    std::println_int(unwrap(n) + 1);

    xs := vec_new[int](4);
    push(xs, 10);
    push(xs, 20);
    std::println_int(len(xs));
    std::println_int(xs[1]);
    return 0;
}
`,
  errors: `use std;

fn main() -> int {
    unused := 1;
    let name: string = 42;
    return name;
}
`,
};

let wasm;
let timer;

function setStatus(kind, text) {
  statusEl.className = 'status ' + kind;
  statusEl.textContent = text;
}

function encode(text) {
  return new TextEncoder().encode(text);
}

function analyze(source) {
  const bytes = encode(source);
  const ptr = wasm.wlel_alloc(bytes.length);
  try {
    new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
    const length = wasm.wlel_analyze(ptr, bytes.length);
    const view = new Uint8Array(wasm.memory.buffer, wasm.wlel_result_ptr(), length);
    return JSON.parse(new TextDecoder().decode(view));
  } finally {
    if (ptr) wasm.wlel_dealloc(ptr, bytes.length);
  }
}

function lineStarts(text) {
  const starts = [0];
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] === '\n') starts.push(i + 1);
  }
  return starts;
}

function offsetOf(text, line, col) {
  const starts = lineStarts(text);
  const start = starts[Math.min(Math.max(line - 1, 0), starts.length - 1)];
  return Math.min(start + Math.max(col - 1, 0), text.length);
}

function render(diagnostics) {
  diagnosticsEl.innerHTML = '';
  if (diagnostics.length === 0) {
    const li = document.createElement('li');
    li.className = 'muted';
    li.textContent = 'No problems.';
    diagnosticsEl.appendChild(li);
    return;
  }
  for (const d of diagnostics) {
    const li = document.createElement('li');
    li.className = d.severity;

    const where = document.createElement('span');
    where.className = 'where';
    where.textContent = `${d.line}:${d.col}`;
    where.title = 'jump to this position';
    where.addEventListener('click', () => {
      const start = offsetOf(sourceEl.value, d.line, d.col);
      const end = offsetOf(sourceEl.value, d.endLine, d.endCol);
      sourceEl.focus();
      sourceEl.setSelectionRange(start, end);
    });

    const sep = document.createTextNode('  ' + d.severity + ': ');
    const msg = document.createElement('span');
    msg.textContent = d.message;

    li.append(where, sep, msg);
    diagnosticsEl.appendChild(li);
  }
}

function run() {
  if (!wasm) return;
  const result = analyze(sourceEl.value);
  render(result.diagnostics);

  const errors = result.diagnostics.filter((d) => d.severity === 'error').length;
  const warnings = result.diagnostics.length - errors;
  if (errors > 0) {
    setStatus('error', `${errors} error${errors === 1 ? '' : 's'}` +
      (warnings ? ` · ${warnings} warning${warnings === 1 ? '' : 's'}` : ''));
  } else if (warnings > 0) {
    setStatus('warning', `ok · ${warnings} warning${warnings === 1 ? '' : 's'}`);
  } else {
    setStatus('ok', 'ok');
  }

  formattedEl.textContent = result.formatted || '';
  formattedWrap.hidden = !result.formatted;
  formatButton.disabled = !result.formatted;
}

function schedule() {
  clearTimeout(timer);
  timer = setTimeout(run, 220);
}

async function boot() {
  try {
    const response = await fetch('wlel.wasm');
    if (!response.ok) throw new Error(`cannot fetch wlel.wasm (${response.status})`);
    let instance;
    if (WebAssembly.instantiateStreaming) {
      ({ instance } = await WebAssembly.instantiateStreaming(response, {}));
    } else {
      const bytes = await response.arrayBuffer();
      ({ instance } = await WebAssembly.instantiate(bytes, {}));
    }
    wasm = instance.exports;
  } catch (err) {
    setStatus('error', 'wasm failed to load');
    diagnosticsEl.innerHTML = '';
    const li = document.createElement('li');
    li.className = 'error';
    li.textContent = String(err.message || err) +
      ' — if you opened this file directly, serve the directory over HTTP ' +
      '(python3 -m http.server) so the module can be fetched.';
    diagnosticsEl.appendChild(li);
    return;
  }

  const version = wasm.wlel_version();
  const bytes = new Uint8Array(wasm.memory.buffer);
  let end = version;
  while (bytes[end] !== 0) end += 1;
  const label = `wlel ${new TextDecoder().decode(bytes.subarray(version, end))}`;
  statusEl.title = label;

  sourceEl.value = EXAMPLES.hello;
  run();
}

sourceEl.addEventListener('input', schedule);
sourceEl.addEventListener('keydown', (event) => {
  if (event.key === 'Tab') {
    event.preventDefault();
    const start = sourceEl.selectionStart;
    const end = sourceEl.selectionEnd;
    sourceEl.setRangeText('    ', start, end, 'end');
    schedule();
  } else if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    run();
  }
});

formatButton.addEventListener('click', () => {
  const result = analyze(sourceEl.value);
  if (result.formatted) {
    sourceEl.value = result.formatted;
    run();
  }
});

exampleButtons.addEventListener('click', (event) => {
  const name = event.target.dataset && event.target.dataset.example;
  if (!name || !EXAMPLES[name]) return;
  sourceEl.value = EXAMPLES[name];
  run();
  sourceEl.focus();
});

boot();
