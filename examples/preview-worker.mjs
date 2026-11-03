import init, { Engine } from './output/wasm/slide_shuttle_wasm.js';

const initialized = init();
let engine;
let document;
let activeRevision = 0;
let imported = false;
let importLosses = [];
let resourcesLoaded = false;

function releaseDocument() {
  document?.free();
  document = undefined;
}
function progress(id, revision, text) {
  self.postMessage({ type: 'progress', id, revision, text });
}
initialized.then(() => {
  engine = new Engine();
  self.postMessage({ type: 'ready' });
}).catch(error => self.postMessage({ type: 'fatal', error: String(error) }));

self.onmessage = async ({ data: message }) => {
  const { id, revision, type, payload = {} } = message;
  try {
    await initialized;
    if (type === 'load' || type === 'compile') {
      activeRevision = revision;
      releaseDocument();
      let project = payload.project;
      if (type === 'load') {
        resourcesLoaded = false;
        engine.clear();
        progress(id, revision, '正在加载字体和图片…');
        for (const font of payload.fonts) engine.addFont(undefined, new Uint8Array(font.bytes));
        for (const image of payload.images ?? []) engine.addImage(image.name, new Uint8Array(image.bytes));
        resourcesLoaded = true;
        imported = payload.kind === 'pptx';
        importLosses = [];
        if (imported) {
          progress(id, revision, '正在读取演示文稿…');
          const result = JSON.parse(engine.importPptx(new Uint8Array(payload.bytes)));
          project = { entry: result.entry, files: result.files };
          importLosses = result.snapshot.losses;
        }
      }
      engine.setFontFallback(payload.fontFallback);
      progress(id, revision, '正在排版…');
      const next = engine.compileProject(project.entry, JSON.stringify(project.files));
      const report = JSON.parse(next.inspect());
      if (!report.valid) {
        next.free();
        const error = new Error(report.diagnostics.filter(d => d.severity === 'error').map(d => `${d.source?.file ? `${d.source.file} · ` : ''}${d.page}/${d.nodeId}: ${d.message}`).join('\n'));
        error.diagnostics = report.diagnostics;
        throw error;
      }
      document = next;
      self.postMessage({ type: 'result', id, revision, value: { project, imported, losses: importLosses, report, stats: JSON.parse(engine.stats()) } });
      return;
    }
    if (revision !== activeRevision) throw new Error('文档已更新，请重试。');
    if (type === 'stats') {
      self.postMessage({ type: 'result', id, revision, value: JSON.parse(engine.stats()) });
      return;
    }
    if (type === 'measure') {
      const value = JSON.parse(engine.measureText(payload.text, JSON.stringify(payload.style ?? {}), payload.width, payload.wrap ?? true));
      self.postMessage({ type: 'result', id, revision, value });
      return;
    }
    if (!document) throw new Error('请先打开有效的演示文稿。');
    let bytes;
    if (type === 'page') bytes = document.renderPng(payload.page, payload.scale ?? 1);
    else if (type === 'pdf') bytes = document.renderPdf();
    else if (type === 'pptx') {
      if (imported) throw new Error('导入稿的 PPTX 重建请使用 CLI 检查差异与损失。');
      bytes = document.exportPptx();
    } else throw new Error(`未知操作：${type}`);
    const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    self.postMessage({ type: 'result', id, revision, value: { bytes: buffer, stats: JSON.parse(engine.stats()), renderStats: JSON.parse(document.stats()) } }, [buffer]);
  } catch (error) {
    self.postMessage({ type: 'error', id, revision, error: String(error), diagnostics: error.diagnostics, resourcesLoaded });
  }
};
