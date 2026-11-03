function cancelled() {
  return Object.assign(new Error('操作已取消'), { code: 'cancelled' });
}

/** Request/version ownership prevents an older render from replacing a new document. */
export class RendererClient {
  constructor({ workerFactory = () => new Worker(new URL('./preview-worker.mjs', import.meta.url), { type: 'module' }), onProgress = () => {} } = {}) {
    this.factory = workerFactory;
    this.onProgress = onProgress;
    this.revision = 0;
    this.sequence = 0;
    this.pending = new Map();
    this.start();
  }
  start() {
    const worker = this.worker = this.factory();
    this.ready = new Promise((resolve, reject) => { this.resolveReady = resolve; this.rejectReady = reject; });
    this.ready.catch(() => {});
    worker.onmessage = ({ data }) => {
      if (worker !== this.worker) return;
      if (data.type === 'ready') { this.resolveReady(); return; }
      if (data.type === 'fatal') { this.fail(new Error(data.error)); return; }
      const pending = this.pending.get(data.id);
      if (!pending) return;
      if (data.revision !== this.revision || pending.revision !== this.revision) {
        this.pending.delete(data.id); clearTimeout(pending.timeout); pending.reject(cancelled()); return;
      }
      if (data.type === 'progress') { this.onProgress(data.text); return; }
      this.pending.delete(data.id); clearTimeout(pending.timeout);
      if (data.type === 'error') pending.reject(Object.assign(new Error(data.error), { diagnostics: data.diagnostics, resourcesLoaded: data.resourcesLoaded }));
      else pending.resolve(data.value);
    };
    worker.onerror = event => { if (worker === this.worker) this.fail(new Error(event.message || '渲染进程发生错误。')); };
  }
  fail(error) {
    this.rejectReady(error);
    for (const p of this.pending.values()) { clearTimeout(p.timeout); p.reject(error); }
    this.pending.clear();
  }
  async request(type, payload = {}, transfer = [], revision = this.revision) {
    await this.ready;
    if (revision !== this.revision) throw cancelled();
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { this.pending.delete(id); reject(new Error('渲染超时，请取消后重试。')); }, 120000);
      this.pending.set(id, { resolve, reject, timeout, revision });
      this.worker.postMessage({ id, revision, type, payload }, transfer);
    });
  }
  open(payload, transfer = []) { const revision = ++this.revision; return this.request('load', payload, transfer, revision); }
  compile(project, fontFallback) { const revision = ++this.revision; return this.request('compile', { project, fontFallback }, [], revision); }
  renderPage(page, scale = 1) { return this.request('page', { page, scale }); }
  exportPdf() { return this.request('pdf'); }
  exportPptx() { return this.request('pptx'); }
  measureText(text, style, width, wrap = true) { return this.request('measure', { text, style, width, wrap }); }
  stats() { return this.request('stats'); }
  cancel() { this.revision++; this.fail(cancelled()); this.worker.terminate(); this.start(); }
  dispose() { this.revision++; this.fail(cancelled()); this.worker.terminate(); }
}
