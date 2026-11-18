// Executed guest/provenance regression for the bounded class-loan NLL compiler
// slice. Runs six scoped positive/negative fixtures on the real wasm32
// browser compiler plus native attoc; full suite is conformance.mjs.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {join} from 'node:path';

const compilerPath = process.argv[2] || 'target/wasm32-unknown-unknown/release/atto_compiler.wasm';
const nativePath = process.env.ATTOC || 'target/release/attoc';
const moduleBytes = await readFile(compilerPath);
const compilerModule = new WebAssembly.Module(moduleBytes);
assert.deepEqual(WebAssembly.Module.imports(compilerModule), [], 'standalone browser compiler');
const api = new WebAssembly.Instance(compilerModule, {}).exports;
assert.equal(api.abi_version(), 2, 'compiler ABI');
const enc = new TextEncoder();
const dec = new TextDecoder();

function compileGuest(source) {
  const bytes = enc.encode(source);
  assert.ok(bytes.length < api.input_capacity(), 'bounded fixture source');
  new Uint8Array(api.memory.buffer, api.input_ptr(), bytes.length).set(bytes);
  const status = api.compile(bytes.length);
  if (status === 0) {
    const wasm = new Uint8Array(api.memory.buffer, api.result_ptr(), api.result_length()).slice();
    assert.deepEqual([...wasm.slice(0, 8)], [0,97,115,109,1,0,0,0]);
    return {ok: true, wasm};
  }
  assert.equal(status, 1, 'diagnostic ABI');
  assert.equal(api.result_length(), 0, 'no emitted guest bytes after a borrow rejection');
  const memory = new Uint8Array(api.memory.buffer);
  const start = api.error_ptr();
  let end = start;
  while (end < memory.length && memory[end]) end++;
  assert.ok(end < memory.length, 'null-terminated diagnostic');
  const diagnostic = dec.decode(memory.slice(start, end));
  return {ok: false, code: diagnostic.split(':')[0], diagnostic};
}

function executeGuest(wasm) {
  let output = '';
  let instance;
  const imported = {
    print_i64: n => { output += String(n); },
    print_bool: n => { output += n ? 'true' : 'false'; },
    print_str: (ptr, len) => { output += dec.decode(new Uint8Array(instance.exports.memory.buffer, ptr, len)); },
    print_newline: () => { output += '\n'; },
    print_f64: n => { output += String(n); },
    actor_spawn: () => { throw new Error('actor execution outside scalar-NLL profile'); },
    actor_send: () => { throw new Error('actor send outside scalar-NLL profile'); },
  };
  const m = new WebAssembly.Module(wasm);
  for (const imp of WebAssembly.Module.imports(m)) {
    assert.equal(imp.module, 'atto');
    assert.ok(Object.hasOwn(imported, imp.name), 'unexpected guest import');
  }
  instance = new WebAssembly.Instance(m, {atto: imported});
  return {value: instance.exports.main(), stdout: output};
}

const pass = [
  "184-nll-reader-then-writer.atto",
  "185-nll-unused-loan-then-move.atto",
  "186-nll-permanent-provenance-owned-owner.atto",
  "187-nll-typed-class-borrow-markers.atto",
  "188-multiply-still-not-dereference.atto"
];
const fail = [
  "184-nll-retain-loan-through-nested-block.atto",
  "185-nll-later-branch-loan-use.atto",
  "186-nll-alias-take-forbidden.atto",
  "187-nll-alias-owned-binding-forbidden.atto",
  "188-nll-alias-reborrow-forbidden.atto",
  "189-borrow-type-shorthand-not-yet-lowered.atto",
  "190-borrow-mut-type-shorthand-not-yet-lowered.atto",
  "191-borrow-type-descriptive-not-yet-lowered.atto",
  "192-borrow-lifetime-positional-not-yet-lowered.atto",
  "193-pointer-type-requires-proof.atto",
  "194-shared-type-requires-proof.atto",
  "195-star-pointer-sigil-disallowed.atto",
  "196-star-pointer-deref-disallowed.atto",
  "197-ampersand-borrow-expression-disallowed.atto",
  "checked-handle-ref-bang.atto",
  "checked-handle-deref-compact.atto",
  "checked-handle-unref-grouped.atto",
  "checked-handle-rt-ref.atto",
  "checked-handle-rt-deref.atto",
  "checked-handle-rt-unref.atto",
  "checked-handle-ref-mut-disallowed.atto",
  "checked-handle-dead-branch-reject.atto",
  "checked-handle-lambda-capture-reject.atto",
  "checked-handle-mixed-rt-bang-reject.atto"
];
let nativeComparisons = 0;
for (const [kind, names] of [['pass', pass], ['fail', fail]]) {
  for (const name of names) {
    const path = join('tests/fixtures', kind, name);
    const source = await readFile(path, 'utf8');
    const result = compileGuest(source);
    const native = spawnSync(nativePath, [path], {encoding: null, maxBuffer: 8 * 1024 * 1024});
    assert.equal(native.error, undefined, name);
    if (kind === 'pass') {
      assert.equal(result.ok, true, name + ': ' + result.diagnostic);
      assert.equal(native.status, 0, name + ': native diagnostic ' + native.stderr);
      assert.deepEqual(native.stdout, Buffer.from(result.wasm), name + ': native/Wasm compiler byte divergence');
      const guest = executeGuest(result.wasm);
      const expectedExit = source.match(/^\/\/ expect-exit: (\d+)$/m);
      assert.ok(expectedExit, name);
      assert.equal(guest.value, BigInt(expectedExit[1]), name);
      const expectedOutput = source.match(/^\/\/ expect-stdout: (.*)$/m);
      assert.equal(guest.stdout, expectedOutput ? expectedOutput[1] + '\n' : '', name);
      nativeComparisons++;
    } else {
      const expectedError = source.match(/^\/\/ expect-error: (\w+)$/m);
      assert.ok(expectedError, name);
      assert.equal(result.ok, false, name + ': invalid source unexpectedly compiled');
      assert.equal(result.code, expectedError[1], name + ': browser diagnostic');
      assert.equal(native.status, 1, name + ': native unexpectedly admitted invalid borrow');
      assert.equal(native.stdout.length, 0, name + ': native emitted bytes after rejection');
      assert.ok(native.stderr.toString().includes(expectedError[1]), name + ': native diagnostic mismatch');
      nativeComparisons++;
    }
  }
}
console.log('PASS class-loan NLL actual compiler/guest: ' + pass.length +
  ' positive executed guests, ' + fail.length +
  ' fail-closed diagnostics, ' + nativeComparisons +
  ' native/Wasm decisions and admitted byte parity');
