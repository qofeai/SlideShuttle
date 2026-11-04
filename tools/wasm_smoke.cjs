// node tools/wasm_smoke.cjs PKG_DIR SOURCE FONT_REGULAR FONT_BOLD [NATIVE_OUTPUT] [QPPT]
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const [pkg, sourcePath, regular, bold, nativeDir, qpptPath] = process.argv.slice(2);
const { Engine } = require(path.resolve(pkg, 'slide_shuttle_wasm.js'));
const engine = new Engine();
engine.addFont('Noto Sans CJK SC', fs.readFileSync(regular));
engine.addFont('Noto Sans CJK SC', fs.readFileSync(bold));
const source = fs.readFileSync(sourcePath, 'utf8');
// Fixtures are authored as fragments in this test, but the public API always sees a project.
function compileSource(engine, xml) {
  const files = {};
  let index = 0;
  xml = xml.replace(/<Slide(?=\s)[\s\S]*?<\/Slide>/g, slide => {
    const name = `fixture/page-${index++}.xml`; files[name] = slide; return `<Slide src="${name}"/>`;
  });
  const entry = path.basename(sourcePath); files[entry] = xml;
  for (const text of Object.values(files)) {
    for (const match of text.matchAll(/<(?:Slide|Library)\s+src="([^"]+)"/g)) {
      if (!(match[1] in files)) files[match[1]] = fs.readFileSync(path.join(path.dirname(sourcePath), match[1]), 'utf8');
    }
  }
  return engine.compileProject(entry, JSON.stringify(files));
}
const measure = JSON.parse(engine.measureText('•', JSON.stringify({ family: 'Noto Sans CJK SC', size: 24, lineHeight: 34 }), undefined, false));
assert(measure.width > 20, 'bullet measurement should explain the former narrow-box benchmark failures');
const compiled = compileSource(engine, source);
assert(compiled.pageCount > 0);
assert.equal(JSON.parse(compiled.inspect()).valid, true);
const animation = compileSource(engine, '<Deck width="80px" height="40px"><Slide id="p"><Shape id="s" style="width:40px;height:40px;background:#FF0000"/><Animate target="s" effect="appear"/></Slide></Deck>');
assert.equal(animation.stepCount(0), 2);
const hidden = animation.renderRgbaStep(0, 0, 1);
const shown = animation.renderRgbaStep(0, 1, 1);
assert.equal(shown.length, 80 * 40 * 4);
assert.deepEqual([...hidden.subarray(0, 4)], [255, 255, 255, 255]);
assert.deepEqual([...shown.subarray(0, 4)], [255, 0, 0, 255]);
assert(Buffer.from(shown).equals(Buffer.from(animation.renderRgba(0, 1))));
assert(Buffer.from(animation.renderPngStep(0, 1, 1)).equals(Buffer.from(animation.renderPng(0, 1))));
assert.throws(() => animation.renderRgbaStep(0, 2, 1), /step index out of range/);
animation.free();
// Continue exercising the source cache with the original document.
compileSource(engine, source).free();
console.error('WASM: checking source');
const report = JSON.parse(compiled.inspect());
const check = { valid: report.valid, pages: report.pageCount };
assert(check.valid, JSON.stringify(check));
assert.equal(compiled.pageCount, check.pages);
const hashes = [];
for (let page = 0; page < check.pages; page++) {
  console.error(`WASM: rendering PNG ${page+1}/${check.pages}`);
  const png = Buffer.from(compiled.renderPng(page, 1));
  const cached = compileSource(engine, source);
  assert(png.equals(Buffer.from(cached.renderPng(page, 1)))); cached.free();
  assert.equal(png.subarray(1, 4).toString(), 'PNG');
  if (nativeDir) assert(png.equals(fs.readFileSync(path.join(nativeDir, `slide-${String(page + 1).padStart(3, '0')}.png`))), `PNG differs on page ${page+1}`);
  hashes.push(crypto.createHash('sha256').update(png).digest('hex'));
}
console.error('WASM: rendering PDF');
const pdf = Buffer.from(compiled.renderPdf());
assert(pdf.subarray(0, 5).equals(Buffer.from('%PDF-')));
console.error('WASM: exporting PPTX');
const pptx = compiled.exportPptx();
assert.equal(Buffer.from(pptx).subarray(0, 2).toString(), 'PK');
console.error('WASM: importing PPTX');
const imported = JSON.parse(engine.importPptx(pptx));
assert.equal(imported.snapshot.pages.length, check.pages);
assert(imported.files[imported.entry].includes('<Slide src="pages/page-1.xml"/>'));
assert.equal(Object.keys(imported.files).length, check.pages + 1, 'import yields an entry plus one file per page');
const statsBeforeEdit = JSON.parse(engine.stats());
assert.equal(statsBeforeEdit.compilations, 3);
assert.equal(statsBeforeEdit.fontBuilds, 1);
// The old immutable document remains renderable after resource invalidation and engine disposal.
const oldPage = Buffer.from(compiled.renderPng(0, 1));
engine.clear();
engine.free();
assert(oldPage.equals(Buffer.from(compiled.renderPng(0, 1))));
assert(Buffer.from(compiled.renderPng(check.pages - 1, 2)).length > 0);
assert(compiled.renderRgba(0, 0.25).length > 0, 'raw frames also survive Engine disposal');
const renderStats = JSON.parse(compiled.stats());
compiled.free();
if (qpptPath) {
  const portable = new Engine();
  assert.equal(portable.compile, undefined, 'the single-source API was removed');
  const bytes = fs.readFileSync(qpptPath);
  const opened = portable.openQppt(bytes);
  assert.equal(opened.pageCount, check.pages);
  assert.equal(JSON.parse(opened.inspect()).valid, true);
  if (nativeDir) assert(Buffer.from(opened.renderPng(0, 1)).equals(fs.readFileSync(path.join(nativeDir, 'slide-001.png'))));
  const project = JSON.parse(portable.importQppt(bytes));
  assert.equal(project.manifest.format, 'qppt');
  assert(!('version' in project.manifest));
  const edited = portable.compileProject(project.entry, JSON.stringify(project.files));
  assert.equal(edited.pageCount, opened.pageCount);
  portable.clear(); portable.free();
  assert(opened.renderRgba(0, 0.5).length > 0);
  opened.free(); edited.free();
}
const preview = new Engine();
preview.addFont('Noto Sans CJK SC', fs.readFileSync(regular));
preview.addFont('Noto Sans CJK SC', fs.readFileSync(bold));
const missingFontSource = source.replaceAll('Noto Sans CJK SC', 'Missing Preview Font');
assert.throws(() => compileSource(preview, missingFontSource), /font face not supplied/);
preview.setFontFallback('Noto Sans CJK SC');
const substituted = compileSource(preview, missingFontSource);
assert(JSON.parse(substituted.inspect()).diagnostics.some(d => d.code === 'font_substitution'));
if (nativeDir) assert(Buffer.from(substituted.renderPng(0, 1)).equals(fs.readFileSync(path.join(nativeDir, 'slide-001.png'))));
preview.setFontFallback(undefined);
assert.throws(() => compileSource(preview, missingFontSource), /font face not supplied/);
const empty = compileSource(preview, '<Deck><Slide id="page-1"><Text id="page-1/object-2" style="width:384px;height:0px"/></Slide></Deck>');
const emptyReport = JSON.parse(empty.inspect());
assert.equal(emptyReport.valid, true);
assert.equal(emptyReport.pages[0].nodes[0].text.lineCount, 0);
assert(empty.renderPng(0, 1).length > 0);
empty.free();
const loadedImport = JSON.parse(preview.importPptx(pptx));
const reimported = preview.compileProject(loadedImport.entry, JSON.stringify(loadedImport.files));
assert.throws(() => reimported.exportPptx(), /baseline and loss decisions/);
assert.equal(reimported.pageCount, check.pages, 'an imported project compiles from its files');
reimported.free();
substituted.free(); preview.free();
console.log(JSON.stringify({ wasm: true, qpptSelfContained: !!qpptPath, nativeRgbaAndClickSteps: true, compiledHandle: true, explicitFontFallback: true, emptyZeroHeightText: true, bulletMeasurement: measure, statsBeforeEdit, renderStats, immutableDocumentAfterEngineFree: true, pages: check.pages, nativePngByteMatch: !!nativeDir, pngSha256: hashes, pdfBytes: pdf.length, pptxBytes: pptx.length }, null, 2));
