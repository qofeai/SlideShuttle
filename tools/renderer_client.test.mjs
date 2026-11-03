import { test } from 'node:test';
import assert from 'node:assert/strict';
import { RendererClient } from '../examples/renderer-client.mjs';

class FakeWorker {
  sent = [];
  terminated = false;
  postMessage(message) { this.sent.push(message); }
  terminate() { this.terminated = true; }
  reply(data) { this.onmessage({ data }); }
}
const tick = () => new Promise(resolve => setImmediate(resolve));
function setup() {
  const workers = [];
  const client = new RendererClient({ workerFactory: () => { const w = new FakeWorker(); workers.push(w); return w; } });
  workers[0].reply({ type: 'ready' });
  return { client, workers };
}

test('old page results cannot replace a newer source', async () => {
  const { client, workers } = setup();
  const w = workers[0];
  const first = client.open({ source: 'old' }); await tick();
  w.reply({ type: 'result', id: w.sent[0].id, revision: 1, value: 'old document' });
  assert.equal(await first, 'old document');
  const oldPage = client.renderPage(0); const rejected = assert.rejects(oldPage, e => e.code === 'cancelled'); await tick();
  const next = client.compile('new'); await tick();
  const oldRequest = w.sent[1], newRequest = w.sent[2];
  w.reply({ type: 'result', id: oldRequest.id, revision: oldRequest.revision, value: 'stale image' });
  w.reply({ type: 'result', id: newRequest.id, revision: newRequest.revision, value: 'new document' });
  await rejected; assert.equal(await next, 'new document'); client.dispose();
});

test('cancel terminates worker and rejects outstanding work', async () => {
  const { client, workers } = setup();
  const pending = client.open({ source: 'slow' }); const rejected = assert.rejects(pending, e => e.code === 'cancelled'); await tick();
  client.cancel(); await rejected;
  assert(workers[0].terminated); assert.equal(workers.length, 2);
  workers[1].reply({ type: 'ready' });
  const next = client.open({ source: 'retry' }); await tick();
  const request = workers[1].sent[0];
  workers[1].reply({ type: 'result', id: request.id, revision: request.revision, value: 'ready' });
  assert.equal(await next, 'ready'); client.dispose();
});

test('worker failure rejects a request instead of leaving a pending promise', async () => {
  const { client, workers } = setup();
  const result = client.compile('bad'); const rejected = assert.rejects(result, /worker failed/); await tick();
  workers[0].onerror({ message: 'worker failed' }); await rejected;
  assert.equal(client.pending.size, 0); client.dispose();
});

test('cancel also releases requests waiting for initialization', async () => {
  const workers = [];
  const client = new RendererClient({ workerFactory: () => { const w = new FakeWorker(); workers.push(w); return w; } });
  const result = client.open({ source: 'early' }); const rejected = assert.rejects(result, e => e.code === 'cancelled');
  client.cancel(); await rejected;
  workers[0].reply({ type: 'ready' });
  assert.equal(workers[1].sent.length, 0); client.dispose();
});
