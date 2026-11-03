import { RendererClient } from './renderer-client.mjs';
import { BUILTIN_FONT_FAMILY, BUILTIN_FONT_BYTES, loadBuiltinFonts } from './builtin-fonts.mjs';

const $ = id => document.getElementById(id);
let workerReady = false;
let busy = false;
let pages = 0;
let page = 0;
let imported = false;
let hasResources = false;
let operationId = 0;
let imageUrl;
let currentPng;
let documentDiagnostics = [];
let fontLoadController;
const client = new RendererClient({ onProgress: text => { $('status').textContent = text; } });

function controls() {
  $('run').disabled = busy || !workerReady;
  $('cancel').disabled = !busy;
  $('previous').disabled = busy || !pages || page === 0;
  $('next').disabled = busy || !pages || page + 1 >= pages;
  $('recompile').disabled = busy || !hasResources;
  for (const id of ['png', 'pdf']) $(id).disabled = busy || !pages;
  $('pptx').disabled = busy || !pages || imported;
  $('page-label').textContent = pages ? `${page + 1} / ${pages}` : '尚未载入';
}
function setBusy(value) {
  busy = value;
  document.body.dataset.busy = String(value);
  controls();
}
function clearDocument() {
  pages = 0; currentPng = undefined;
  documentDiagnostics = [];
  if (imageUrl) URL.revokeObjectURL(imageUrl);
  imageUrl = undefined;
  $('slide').removeAttribute('src'); $('slide').hidden = true;
  $('placeholder').hidden = false;
  $('losses').hidden = true;
  $('font-notice').hidden = true;
  $('diagnostics').textContent = '';
  controls();
}
function showError(error, operation) {
  if (operation !== operationId || error.code === 'cancelled') return;
  clearDocument();
  if (error.resourcesLoaded) hasResources = true;
  document.body.dataset.state = 'error';
  $('status').textContent = error.message || String(error);
  if (error.diagnostics) $('diagnostics').textContent = JSON.stringify(error.diagnostics, null, 2);
}
function applyDocument(result) {
  hasResources = true;
  pages = result.report.pageCount; page = 0; imported = result.imported;
  $('xml').value = result.source;
  documentDiagnostics = result.report.diagnostics;
  const substitutions = documentDiagnostics.filter(d => d.code === 'font_substitution');
  if (substitutions.length) {
    $('font-notice').textContent = `有 ${substitutions.length} 处字体或字形样式使用了替代，文字换行可能与原稿不同。可在“字体与图片”中补充原字体，再重新打开。详情见诊断信息。`;
    $('font-notice').hidden = false;
  }
  $('diagnostics').textContent = JSON.stringify({ stats: result.stats, diagnostics: result.report.diagnostics }, null, 2);
  $('canvas').style.aspectRatio = `${result.report.width} / ${result.report.height}`;
  $('pptx-note').hidden = !imported;
  if (result.losses.length) {
    $('losses').hidden = false;
    $('loss-summary').textContent = `导入重建报告了 ${result.losses.length} 项限制，部分效果可能与原文件不同。`;
    $('loss-detail').textContent = result.losses.slice(0, 40).map(l => `${l.page} · ${l.object}: ${l.reason}`).join('\n') + (result.losses.length > 40 ? '\n…这里只显示前 40 项。' : '');
  }
  window.dispatchEvent(new CustomEvent('slideshuttle:compiled', { detail: { pageCount: pages, imported, stats: result.stats } }));
}
async function showPage(index, operation) {
  $('status').textContent = `正在绘制第 ${index + 1} 页…`;
  const result = await client.renderPage(index);
  if (operation !== operationId) return;
  const nextUrl = URL.createObjectURL(new Blob([result.bytes], { type: 'image/png' }));
  const previousUrl = imageUrl;
  imageUrl = nextUrl;
  $('slide').src = nextUrl;
  await $('slide').decode();
  if (operation !== operationId) return;
  if (previousUrl) URL.revokeObjectURL(previousUrl);
  $('slide').hidden = false; $('placeholder').hidden = true;
  currentPng = result.bytes; page = index;
  document.body.dataset.state = 'ready';
  $('status').textContent = `已载入 ${pages} 页。翻页复用同一份排版结果。`;
  $('diagnostics').textContent = JSON.stringify({ stats: result.stats, renderStats: result.renderStats, diagnostics: documentDiagnostics }, null, 2);
  window.dispatchEvent(new CustomEvent('slideshuttle:rendered', { detail: { page, pageCount: pages, stats: result.stats, renderStats: result.renderStats } }));
  controls();
}
async function selectedFiles(input, maxFileSize, label) {
  const files = [...input.files];
  for (const file of files) if (file.size > maxFileSize) throw new Error(`${label} ${file.name} 超过大小限制。`);
  return Promise.all(files.map(async file => {
    return { name: file.webkitRelativePath || file.name, bytes: await file.arrayBuffer() };
  }));
}
function save(bytes, mime, name) {
  const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
  const link = document.createElement('a'); link.href = url; link.download = name; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function selectionChanged() {
  controls();
  if (busy || !workerReady) return;
  const filename = $('source').files[0]?.name || '中文示例';
  $('status').textContent = `已选择 ${filename}，可以直接打开。${$('fonts').files.length ? `将加载 ${$('fonts').files.length} 个补充字体文件。` : '已提供内置中文字体。'}`;
}
$('fonts').onchange = selectionChanged;
$('source').onchange = () => { $('filename').textContent = $('source').files[0]?.name || '中文示例'; selectionChanged(); };
$('run').onclick = async () => {
  const operation = ++operationId;
  hasResources = false;
  const controller = fontLoadController = new AbortController();
  setBusy(true); clearDocument(); document.body.dataset.state = 'loading';
  try {
    const resourceFiles = [...$('fonts').files, ...$('images').files];
    if (resourceFiles.length > 510 || resourceFiles.reduce((n, f) => n + f.size, BUILTIN_FONT_BYTES) > 256 * 1024 * 1024) throw new Error('字体与图片（含内置字体）合计超过 256 MiB 或 512 个文件。');
    $('status').textContent = '正在加载内置中文字体（首次约 38 MiB）…';
    const defaults = await loadBuiltinFonts(controller.signal);
    const fonts = [...await selectedFiles($('fonts'), 64 * 1024 * 1024, '字体'), ...defaults];
    const images = await selectedFiles($('images'), 32 * 1024 * 1024, '图片');
    if (new Set(images.map(i => i.name)).size !== images.length) throw new Error('图片文件名重复，请使用唯一名称。');
    if (fonts.concat(images).reduce((n, f) => n + f.bytes.byteLength, 0) > 256 * 1024 * 1024) throw new Error('字体与图片合计超过 256 MiB。');
    const sourceFile = $('source').files[0];
    const kind = sourceFile?.name.toLowerCase().endsWith('.pptx') ? 'pptx' : 'xml';
    if (sourceFile && !/\.(pptx|xml)$/i.test(sourceFile.name)) throw new Error('请选择 XML 或 PPTX 演示文稿。');
    if (sourceFile && sourceFile.size > (kind === 'pptx' ? 64 : 4) * 1024 * 1024) throw new Error('演示文稿超过大小限制。');
    const bytes = kind === 'pptx' ? await sourceFile.arrayBuffer() : undefined;
    const source = kind === 'xml' ? (sourceFile ? await sourceFile.text() : $('xml').value) : undefined;
    if (kind === 'xml') $('xml').value = source;
    if (operation !== operationId) return;
    const transfer = fonts.concat(images).map(file => file.bytes);
    if (bytes) transfer.push(bytes);
    const fontFallback = $('font-fallback').checked ? BUILTIN_FONT_FAMILY : undefined;
    const result = await client.open({ fonts, images, kind, bytes, source, fontFallback }, transfer);
    if (operation !== operationId) return;
    applyDocument(result);
    await showPage(0, operation);
  } catch (error) { controller.abort(); showError(error, operation); }
  finally { if (operation === operationId) { fontLoadController = undefined; setBusy(false); } }
};
$('recompile').onclick = async () => {
  const operation = ++operationId;
  const source = $('xml').value;
  setBusy(true); clearDocument(); document.body.dataset.state = 'loading';
  try {
    const result = await client.compile(source, $('font-fallback').checked ? BUILTIN_FONT_FAMILY : undefined);
    if (operation !== operationId) return;
    applyDocument(result); await showPage(0, operation);
  } catch (error) { showError(error, operation); }
  finally { if (operation === operationId) setBusy(false); }
};
for (const [id, delta] of [['previous', -1], ['next', 1]]) $(id).onclick = async () => {
  const operation = operationId;
  setBusy(true);
  try { await showPage(page + delta, operation); } catch (error) { showError(error, operation); }
  finally { if (operation === operationId) setBusy(false); }
};
$('cancel').onclick = () => {
  operationId++; fontLoadController?.abort(); fontLoadController = undefined;
  client.cancel(); workerReady = false; hasResources = false; clearDocument(); setBusy(false);
  document.body.dataset.state = 'cancelled'; $('status').textContent = '已取消，可重新打开文件。';
  client.ready.then(() => { workerReady = true; controls(); }).catch(error => { $('status').textContent = error.message; });
};
$('png').onclick = () => save(currentPng, 'image/png', `SlideShuttle-${page + 1}.png`);
for (const [id, method, mime, suffix] of [['pdf', 'exportPdf', 'application/pdf', 'pdf'], ['pptx', 'exportPptx', 'application/vnd.openxmlformats-officedocument.presentationml.presentation', 'pptx']]) $(id).onclick = async () => {
  const operation = operationId; setBusy(true); $('status').textContent = '正在导出…';
  try { const result = await client[method](); if (operation === operationId) { save(result.bytes, mime, `SlideShuttle.${suffix}`); $('status').textContent = '导出完成。'; } }
  catch (error) { showError(error, operation); }
  finally { if (operation === operationId) setBusy(false); }
};

try {
  const response = await fetch('./demo.xml');
  if (!response.ok) throw new Error('无法加载中文示例。');
  $('xml').value = await response.text();
  await client.ready; workerReady = true;
  selectionChanged();
} catch (error) { $('status').textContent = error.message || String(error); }
window.addEventListener('beforeunload', () => client.dispose());
