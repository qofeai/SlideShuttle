import { test } from 'node:test';
import assert from 'node:assert/strict';

let sequence = 0;
const freshLoader = () => import(`../examples/builtin-fonts.mjs?test=${sequence++}`);
const success = () => Promise.resolve(new Response(new Uint8Array([1, 2, 3])));

test('transferring built-in fonts does not detach cached bytes for the next document', async () => {
  const previous = globalThis.fetch;
  let requests = 0;
  globalThis.fetch = () => { requests++; return success(); };
  try {
    const { loadBuiltinFonts } = await freshLoader();
    const first = await loadBuiltinFonts();
    structuredClone(first, { transfer: first.map(font => font.bytes) });
    assert.equal(first[0].bytes.byteLength, 0);
    const next = await loadBuiltinFonts();
    assert.equal(requests, 2);
    assert.deepEqual([...new Uint8Array(next[0].bytes)], [1, 2, 3]);
  } finally { globalThis.fetch = previous; }
});

test('a failed built-in font download can be retried', async () => {
  const previous = globalThis.fetch;
  globalThis.fetch = () => Promise.resolve(new Response('', { status: 503 }));
  try {
    const { loadBuiltinFonts } = await freshLoader();
    await assert.rejects(loadBuiltinFonts(), /内置字体加载失败（503）/);
    globalThis.fetch = success;
    const fonts = await loadBuiltinFonts();
    assert.equal(fonts.length, 2);
    assert.equal(fonts[1].bytes.byteLength, 3);
  } finally { globalThis.fetch = previous; }
});

test('cancelling a font download releases the cache for a later load', async () => {
  const previous = globalThis.fetch;
  globalThis.fetch = (_, { signal }) => new Promise((_, reject) => {
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
  });
  try {
    const { loadBuiltinFonts } = await freshLoader();
    const controller = new AbortController();
    const result = loadBuiltinFonts(controller.signal);
    controller.abort();
    await assert.rejects(result, { name: 'AbortError' });
    globalThis.fetch = success;
    const fonts = await loadBuiltinFonts();
    assert.equal(fonts[0].bytes.byteLength, 3);
  } finally { globalThis.fetch = previous; }
});
