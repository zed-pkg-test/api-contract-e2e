// Native vs independent wasm32 browser compiler lifetime preflight parity.
// No external packages, secrets, cross-org checkout or production borrow ABI.
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const wasmFile = process.argv[2] || 'target/wasm32-unknown-unknown/release/atto_compiler.wasm';
const attoc = process.env.ATTOC || 'target/release/attoc';
const compilerModule = new WebAssembly.Module(readFileSync(wasmFile));
assert.deepEqual(WebAssembly.Module.imports(compilerModule), [], 'standalone compiler must have no imports');
const api = new WebAssembly.Instance(compilerModule, {}).exports;
assert.equal(api.abi_version(), 2);
const enc = new TextEncoder();
const dec = new TextDecoder();
const dir = mkdtempSync(join(tmpdir(), 'atto-lt-parity-'));
const input = join(dir, 'case.atto');
let comparisons = 0;
let rejections = 0;
let guestPasses = 0;

function browser(source) {
  const bytes = enc.encode(source);
  assert.ok(bytes.length < api.input_capacity(), 'bounded browser input');
  new Uint8Array(api.memory.buffer, api.input_ptr(), bytes.length).set(bytes);
  const status = api.compile(bytes.length);
  if (status === 0) {
    const guest = new Uint8Array(api.memory.buffer, api.result_ptr(), api.result_length()).slice();
    return {ok: true, guest};
  }
  assert.equal(status, 1, 'compiler must emit an error status or guest bytes');
  assert.equal(api.result_length(), 0, 'rejected program must emit zero bytes');
  const mem = new Uint8Array(api.memory.buffer);
  const start = api.error_ptr();
  let end = start;
  while (end < mem.length && mem[end] !== 0) end++;
  assert.ok(end < mem.length, 'bounded null-terminated diagnostic');
  const code = dec.decode(mem.slice(start, end)).match(/\bE_[A-Z_]+\b/)?.[0];
  assert.ok(code, 'browser diagnostic code');
  return {ok: false, code};
}

function compare(source, expected = null) {
  writeFileSync(input, source);
  const wasm = browser(source);
  const native = spawnSync(attoc, [input], {maxBuffer: 4 * 1024 * 1024});
  assert.equal(native.error, undefined, 'native compiler spawned');
  const ok = native.status === 0;
  assert.equal(ok, wasm.ok, 'different accept/reject decisions for: ' + source);
  if (ok) {
    assert.deepEqual(native.stdout, Buffer.from(wasm.guest), 'emitted guest bytes differ');
    assert.equal(expected, 'guest', 'unexpected source accepted: ' + source);
    assert.deepEqual([...wasm.guest.slice(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
    const mod = new WebAssembly.Module(wasm.guest);
    assert.equal(WebAssembly.Module.imports(mod).length, 0, 'simple guest imports');
    const instance = new WebAssembly.Instance(mod, {});
    assert.equal(instance.exports.main(), 42n);
    guestPasses++;
  } else {
    assert.equal(native.status, 1, 'native must report language failure');
    assert.equal(native.stdout.length, 0, 'native must not output guest bytes');
    const code = native.stderr.toString().match(/\bE_[A-Z_]+\b/)?.[0];
    assert.equal(code, wasm.code, 'native/browser error mismatch: ' + source);
    if (expected) assert.equal(code, expected, 'unexpected error code: ' + source);
    rejections++;
  }
  comparisons++;
}

try {
  const head = p => `fnc f<${p}>(): int { return 42; }`;
  for (const p of [
    'lt!a', 'lt! a', 'lt! α', 'lifetime!a', 'lifetime! a',
    'T, lt! a', 'lt! a, T', 'lt! a, lifetime! b',
    'lt!a,lt!b', 'lt! a, lt! a', 'lifetime! a, lt! a',
    'lt a', 'lifetime a', 'lt ! a', 'lt\n! a', 'lifetime\t!a',
    'lt/**/! a', 'lt!=a', 'lt=a', 'lt!', 'lt! a,',
    'lt! self', 'lifetime! return', 'lt! a, lt! b, lt! a'
  ]) compare(head(p));
  for (const source of [
    'define class View<lt! a> as end',
    'define actor Worker<lifetime! a> as end',
    'define enum State<lt! a> as Open end',
    'define class View as fnc f<lt! a>(): int { return 42; } end',
    'fnc f(x: Borrow<int, lt=a>): int { return 1; }',
    'fnc f(x: BorrowMut<int, lt=a>): int { return 1; }',
    'pub fnc main(): int { return 42; } fnc hidden<lt! a>(): int { return 1; }',
    'lt! a;',
    'pub fnc main(): int { val x = lt! a; return 42; }',
  ]) compare(source);
  for (const name of ['lt', 'lifetime']) {
    compare(`pub fnc main(): int { val ${name} = 42; return ${name}; }`, 'guest');
  }
  // Reproducible adversarial corpus, no random seed or external input.
  const atoms = ['lt', 'lifetime', '!', 'a', 'T', ',', ' ', '\n', '(', ')', '=', '>', '<', '/*x*/', 'α', "'a'"];
  let state = 0x7A77B1;
  for (let i = 0; i < 256; i++) {
    let params = '';
    for (let n = 0; n <= i % 17; n++) {
      state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
      params += atoms[state % atoms.length];
    }
    compare(head(params));
  }
  console.log(`PASS lifetime differential: ${comparisons} native/Wasm decisions, ${rejections} fail-closed, ${guestPasses} executed guests, byte parity`);
} finally {
  rmSync(dir, {recursive: true, force: true});
}
