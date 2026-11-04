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
let presenting = false;
let presentationPage = 0;
let presentationStep = 0;
let pageReports = [];
let deckWidth = 0;
let deckHeight = 0;
let builtinFontsAvailable = true;
/** Sources being previewed: entry path plus every project file, edited one file at a time. */
let project = { entry: 'demo.xml', files: {} };
let currentFile = 'demo.xml';
const client = new RendererClient({ onProgress: text => { $('status').textContent = text; } });

function controls() {
  $('run').disabled = busy || !workerReady;
  $('cancel').disabled = !busy;
  $('previous').disabled = busy || !pages || page === 0;
  $('next').disabled = busy || !pages || page + 1 >= pages;
  $('recompile').disabled = busy || !hasResources;
  for (const id of ['png', 'pdf']) $(id).disabled = busy || !pages;
  $('pptx').disabled = busy || !pages || imported;
  $('present').disabled = busy || !pages;
  $('show-previous').disabled = busy || (presentationPage === 0 && presentationStep === 0);
  $('show-next').disabled = busy || (presentationPage + 1 >= pages && presentationStep + 1 >= (pageReports[presentationPage]?.stepCount ?? 1));
  $('show-png').disabled = busy;
  $('page-label').textContent = pages ? `${page + 1} / ${pages}` : '尚未载入';
}
function setBusy(value) {
  busy = value;
  document.body.dataset.busy = String(value);
  controls();
}
function clearDocument() {
  exitPresentation();
  pages = 0; currentPng = undefined;
  pageReports = [];
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
function showProject(next, file = next.entry) {
  project = next; currentFile = file in next.files ? file : next.entry;
  const names = [next.entry, ...Object.keys(next.files).filter(name => name !== next.entry).sort()];
  $('file').replaceChildren(...names.map(name => new Option(name, name, false, name === currentFile)));
  $('file').hidden = names.length < 2;
  $('xml').value = next.files[currentFile] ?? '';
}
function editedProject() {
  return { entry: project.entry, files: { ...project.files, [currentFile]: $('xml').value } };
}
function applyDocument(result) {
  hasResources = true;
  pages = result.report.pageCount; page = 0; imported = result.imported;
  pageReports = result.report.pages;
  deckWidth = result.report.width; deckHeight = result.report.height;
  showProject(result.project, currentFile);
  documentDiagnostics = result.report.diagnostics;
  const substitutions = documentDiagnostics.filter(d => d.code === 'font_substitution');
  if (substitutions.length) {
    $('font-notice').textContent = `有 ${substitutions.length} 处字体或字形样式使用了替代，文字换行可能与原稿不同。可在“字体与图片”中补充原字体，再重新打开。详情见诊断信息。`;
    $('font-notice').hidden = false;
  }
  $('diagnostics').textContent = JSON.stringify({ stats: result.stats, diagnostics: result.report.diagnostics }, null, 2);
  $('canvas').style.aspectRatio = `${result.report.width} / ${result.report.height}`;
  $('stage').style.setProperty('--slide-ratio', String(deckWidth / deckHeight));
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
  const filename = $('filename').textContent;
  $('status').textContent = `已选择 ${filename}，可以直接打开。${$('fonts').files.length ? `将加载 ${$('fonts').files.length} 个补充字体文件。` : '已提供内置中文字体。'}`;
}
$('fonts').onchange = selectionChanged;
$('source').onchange = () => { $('folder').value = ''; $('filename').textContent = $('source').files[0]?.name || '中文示例'; selectionChanged(); };
$('folder').onchange = () => {
  $('source').value = '';
  $('filename').textContent = $('folder').files[0]?.webkitRelativePath.split('/')[0] || '中文示例';
  selectionChanged();
};
$('xml').oninput = () => { project.files[currentFile] = $('xml').value; };
$('file').onchange = () => { project.files[currentFile] = $('xml').value; showProject(project, $('file').value); };
/** Split a selected folder into sources, images and fonts keyed by path inside the folder. */
async function folderProject(files) {
  const inside = file => file.webkitRelativePath.split('/').slice(1).join('/');
  const sources = {}; const images = []; const fonts = [];
  let manifest;
  for (const file of files) {
    const name = inside(file);
    if (name === 'manifest.json') manifest = JSON.parse(await file.text());
    else if (/\.xml$/i.test(name)) {
      if (file.size > 4 * 1024 * 1024) throw new Error(`源文件 ${name} 超过 4 MiB。`);
      sources[name] = await file.text();
    } else if (/\.(png|jpe?g|svg)$/i.test(name)) images.push({ name, bytes: await file.arrayBuffer() });
    else if (/\.(ttf|otf|ttc)$/i.test(name)) fonts.push({ name, bytes: await file.arrayBuffer() });
  }
  const entry = 'deck.xml' in sources ? 'deck.xml' : Object.keys(sources).find(name => !name.includes('/') && /^\s*(<\?xml[^>]*>\s*)?<Deck[\s>]/.test(sources[name]));
  if (!entry) throw new Error('文件夹中没有 deck.xml 或根元素为 <Deck> 的入口文件。');
  return { project: { entry, files: sources }, images, fonts, manifest };
}
$('run').onclick = async () => {
  const operation = ++operationId;
  hasResources = false;
  const controller = fontLoadController = new AbortController();
  setBusy(true); clearDocument(); document.body.dataset.state = 'loading';
  try {
    const folder = $('folder').files.length ? await folderProject([...$('folder').files]) : undefined;
    const sourceFile = folder ? undefined : $('source').files[0];
    const kind = sourceFile ? (sourceFile.name.toLowerCase().endsWith('.qppt') ? 'qppt' : 'pptx') : 'project';
    if (sourceFile && !/\.(pptx|qppt)$/i.test(sourceFile.name)) throw new Error('请选择 QPPT 或 PPTX 演示文稿；创作时可打开项目文件夹。');
    if (sourceFile && sourceFile.size > (kind === 'qppt' ? 256 : 64) * 1024 * 1024) throw new Error('演示文稿超过大小限制。');
    builtinFontsAvailable = kind !== 'qppt';
    const resourceFiles = [...$('fonts').files, ...$('images').files];
    if (resourceFiles.length > 510 || resourceFiles.reduce((n, f) => n + f.size, BUILTIN_FONT_BYTES) > 256 * 1024 * 1024) throw new Error('字体与图片（含内置字体）合计超过 256 MiB 或 512 个文件。');
    $('status').textContent = kind === 'qppt' ? '正在读取 QPPT 文稿与内嵌资源…' : '正在加载内置中文字体（首次约 38 MiB）…';
    const defaults = builtinFontsAvailable ? await loadBuiltinFonts(controller.signal) : [];
    const fonts = [...await selectedFiles($('fonts'), 64 * 1024 * 1024, '字体'), ...(folder?.fonts ?? []), ...defaults];
    const images = [...await selectedFiles($('images'), 32 * 1024 * 1024, '图片'), ...(folder?.images ?? [])];
    if (new Set(images.map(i => i.name)).size !== images.length) throw new Error('图片文件名重复，请使用唯一名称。');
    if (fonts.concat(images).reduce((n, f) => n + f.bytes.byteLength, 0) > 256 * 1024 * 1024) throw new Error('字体与图片合计超过 256 MiB。');
    const bytes = sourceFile ? await sourceFile.arrayBuffer() : undefined;
    if (kind === 'project') {
      if (folder) showProject(folder.project);
      else showProject(editedProject(), currentFile);
    }
    const sources = kind === 'project' ? editedProject() : undefined;
    if (operation !== operationId) return;
    const transfer = fonts.concat(images).map(file => file.bytes);
    if (bytes) transfer.push(bytes);
    const fontFallback = $('font-fallback').checked && builtinFontsAvailable ? BUILTIN_FONT_FAMILY : undefined;
    const result = await client.open({ fonts, images, kind, bytes, project: sources, fontFallback, manifest: folder?.manifest }, transfer);
    if (operation !== operationId) return;
    applyDocument(result);
    await showPage(0, operation);
  } catch (error) { controller.abort(); showError(error, operation); }
  finally { if (operation === operationId) { fontLoadController = undefined; setBusy(false); } }
};
$('recompile').onclick = async () => {
  const operation = ++operationId;
  const sources = editedProject();
  setBusy(true); clearDocument(); document.body.dataset.state = 'loading';
  try {
    const result = await client.compile(sources, $('font-fallback').checked && builtinFontsAvailable ? BUILTIN_FONT_FAMILY : undefined);
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
function exitPresentation() {
  if (!presenting) return;
  presenting = false;
  document.body.dataset.presenting = 'false';
  $('frame').hidden = true;
  // Drop the backing surface as soon as the client leaves presentation mode.
  $('frame').width = $('frame').height = 0;
  $('slide').hidden = !currentPng;
  $('show-controls').hidden = $('show-help').hidden = true;
  if (document.fullscreenElement === $('stage')) document.exitFullscreen?.().catch(() => {});
  $('present').focus();
}
async function showFrame(index, step) {
  const operation = operationId;
  setBusy(true);
  try {
    // Bound the display surface to the viewport and the document's native resolution.
    const scale = Math.min(1, Math.max(0.1, Math.min(innerWidth / deckWidth, (innerHeight - 110) / deckHeight) * (devicePixelRatio || 1)));
    const result = await client.renderFrame(index, step, scale);
    if (!presenting || operation !== operationId) return;
    const frame = $('frame');
    frame.width = result.width; frame.height = result.height;
    frame.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(result.bytes), result.width, result.height), 0, 0);
    presentationPage = index; presentationStep = step;
    frame.setAttribute('aria-label', `第 ${index + 1} 页，第 ${step} 步`);
    $('step-label').textContent = `${index + 1} / ${pages} · 步骤 ${step} / ${(pageReports[index]?.stepCount ?? 1) - 1}`;
    window.dispatchEvent(new CustomEvent('slideshuttle:frame', { detail: { page: index, step, width: result.width, height: result.height, stats: result.stats } }));
  } catch (error) { exitPresentation(); showError(error, operation); }
  finally { if (operation === operationId) setBusy(false); }
}
function advancePresentation(delta) {
  if (!presenting || busy) return;
  let index = presentationPage, step = presentationStep + delta;
  if (step >= (pageReports[index]?.stepCount ?? 1)) { index++; step = 0; }
  else if (step < 0) { index--; step = (pageReports[index]?.stepCount ?? 1) - 1; }
  if (index >= 0 && index < pages) void showFrame(index, step);
}
$('present').onclick = () => {
  if (busy || !pages) return;
  presenting = true; presentationPage = page; presentationStep = 0;
  document.body.dataset.presenting = 'true';
  $('slide').hidden = true; $('frame').hidden = false;
  $('frame').focus();
  $('show-controls').hidden = $('show-help').hidden = false;
  $('stage').requestFullscreen?.().catch(() => {}); // The fixed stage also works without this API.
  void showFrame(page, 0);
};
$('show-exit').onclick = exitPresentation;
$('show-previous').onclick = () => advancePresentation(-1);
$('show-next').onclick = $('canvas').onclick = () => advancePresentation(1);
$('show-png').onclick = async () => {
  const operation = operationId, index = presentationPage, step = presentationStep;
  setBusy(true);
  try {
    const result = await client.exportStepPng(index, step);
    if (operation === operationId && presenting) save(result.bytes, 'image/png', `SlideShuttle-${index + 1}-step-${step}.png`);
  } catch (error) { showError(error, operation); }
  finally { if (operation === operationId) setBusy(false); }
};
document.addEventListener('keydown', event => {
  if (!presenting) return;
  if (event.key === 'Escape') { event.preventDefault(); exitPresentation(); }
  else if (!(event.key === ' ' && event.target.closest('button')) && ['ArrowRight', 'ArrowDown', ' ', 'PageDown', 'ArrowLeft', 'ArrowUp', 'PageUp'].includes(event.key)) {
    event.preventDefault(); advancePresentation(['ArrowLeft', 'ArrowUp', 'PageUp'].includes(event.key) ? -1 : 1);
  }
});
document.addEventListener('fullscreenchange', () => { if (!document.fullscreenElement) exitPresentation(); });
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
  const entry = await response.text();
  const files = { 'demo.xml': entry };
  const references = [...entry.matchAll(/<(?:Slide|Library)\s+src="([^"]+)"/g)].map(match => match[1]);
  await Promise.all(references.map(async name => {
    const result = await fetch(`./${name}`);
    if (!result.ok) throw new Error(`无法加载示例文件 ${name}。`);
    files[name] = await result.text();
  }));
  showProject({ entry: 'demo.xml', files });
  await client.ready; workerReady = true;
  selectionChanged();
} catch (error) { $('status').textContent = error.message || String(error); }
window.addEventListener('beforeunload', () => client.dispose());
