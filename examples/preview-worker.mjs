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
        imported = false;
        importLosses = [];
        progress(id, revision, '正在加载字体和图片…');
        if (payload.kind === 'qppt') {
          const result = JSON.parse(engine.importQppt(new Uint8Array(payload.bytes)));
          project = { entry: result.entry, files: result.files };
          imported = result.manifest.imported ?? false;
          importLosses = result.manifest.losses ?? [];
        }
        for (const font of payload.fonts ?? []) engine.addFont(undefined, new Uint8Array(font.bytes));
        for (const image of payload.images ?? []) engine.addImage(image.name, new Uint8Array(image.bytes));
        resourcesLoaded = true;
        if (payload.kind === 'pptx') {
          imported = true;
          progress(id, revision, '正在读取演示文稿…');
          const result = JSON.parse(engine.importPptx(new Uint8Array(payload.bytes)));
          project = { entry: result.entry, files: result.files };
          importLosses = result.snapshot.losses;
        }
        if (payload.manifest?.imported) { imported = true; importLosses = payload.manifest.losses ?? []; }
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
    let dimensions;
    if (type === 'page') bytes = document.renderPng(payload.page, payload.scale ?? 1);
    else if (type === 'step-png') bytes = document.renderPngStep(payload.page, payload.step, payload.scale ?? 1);
    else if (type === 'frame') {
      const scale = payload.scale ?? 1;
      bytes = document.renderRgbaStep(payload.page, payload.step, scale);
      // Match the renderer's f32 multiplication, including fractional scales.
      dimensions = { width: Math.ceil(Math.fround(document.width * Math.fround(scale))), height: Math.ceil(Math.fround(document.height * Math.fround(scale))) };
    }
    else if (type === 'pdf') bytes = document.renderPdf();
    else if (type === 'pptx') {
      if (imported) throw new Error('导入稿的 PPTX 重建请使用 CLI 检查差异与损失。');
      bytes = document.exportPptx();
    } else throw new Error(`未知操作：${type}`);
    // wasm-bindgen returns an owned, transferable Uint8Array. Avoid another full-frame copy.
    const buffer = bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength
      ? bytes.buffer : bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    self.postMessage({ type: 'result', id, revision, value: { bytes: buffer, ...dimensions, stats: JSON.parse(engine.stats()), renderStats: JSON.parse(document.stats()) } }, [buffer]);
  } catch (error) {
    self.postMessage({ type: 'error', id, revision, error: String(error), diagnostics: error.diagnostics, resourcesLoaded });
  }
};
