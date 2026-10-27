// Actual Node WASM identity execution only; this does not execute the warehouse.
import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const [wasmPath, nativeHex] = process.argv.slice(2);
assert.match(nativeHex ?? '', /^[0-9a-f]{64}$/);
const module = await WebAssembly.compile(await readFile(wasmPath));
assert.deepEqual(WebAssembly.Module.imports(module), [], 'unexpected host imports');
const instance = await WebAssembly.instantiate(module, {});
const bytes = Array.from({ length: 32 }, (_, index) => {
  const value = instance.exports.identity_byte(index);
  assert.ok(Number.isInteger(value) && value >= 0 && value <= 255, 'identity admission/index failure');
  return value;
});
assert.equal(instance.exports.identity_byte(32), 256, 'missing failure sentinel');
assert.equal(Buffer.from(bytes).toString('hex'), nativeHex, 'native/WASM identities differ');
console.log('Node WASM: all 32 identity bytes match native');
