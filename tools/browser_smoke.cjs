// Real Chromium + real WASM + real Worker; no mocked rendering.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawn, execFileSync } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || './browser/node_modules/playwright');
const output = path.join(root, 'examples/output/browser-v2');
fs.mkdirSync(output, { recursive: true });
const emptyTextFixture = path.join(output, 'empty-text-regression.pptx');
execFileSync('python3', [path.join(root, 'tools/create_empty_text_fixture.py'), '--base', path.join(root, 'examples/output/SlideShuttle-demo.pptx'), '--out', emptyTextFixture]);
const themeFixture = path.join(output, 'theme-regression.pptx');
const themeExpected = path.join(output, 'theme-expected.xml');
const themeNative = path.join(output, 'theme-native');
execFileSync('python3', [path.join(root, 'tools/create_theme_fixture.py'), '--base', path.join(root, 'examples/output/SlideShuttle-demo.pptx'), '--out', themeFixture, '--expected', themeExpected]);
fs.rmSync(themeNative, { recursive: true, force: true });
execFileSync(path.join(root, 'target/debug/slide-shuttle'), ['render', themeExpected, '--out', themeNative, '--font', path.join(root, 'examples/fonts/NotoSansCJK-Regular.ttc'), '--font', path.join(root, 'examples/fonts/NotoSansCJK-Bold.ttc')]);
const python = spawn('python3', ['-u', '-c', `
import http.server,sys
class Handler(http.server.SimpleHTTPRequestHandler):
 def __init__(self,*args,**kwargs):super().__init__(*args,directory=sys.argv[1],**kwargs)
 def log_message(self,*args):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
print(server.server_port,flush=True)
server.serve_forever()
`, path.join(root, 'examples')], { stdio: ['ignore', 'pipe', 'pipe'] });

async function main() {
  const port = await new Promise((resolve, reject) => {
    let text = '';
    const timeout = setTimeout(() => reject(new Error('preview server startup timed out')), 10000);
    python.stdout.on('data', chunk => { text += chunk; if (text.includes('\n')) { clearTimeout(timeout); resolve(Number(text.split('\n')[0])); } });
    python.once('error', reject);
    python.once('exit', code => { clearTimeout(timeout); reject(new Error(`preview server exited ${code}`)); });
  });
  const origin = `http://127.0.0.1:${port}`;
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ acceptDownloads: true, viewport: { width: 1440, height: 1050 } });
  const page = await context.newPage();
  const errors = [], externalRequests = [], fontRequests = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { const url = new URL(request.url()); if (url.pathname.startsWith('/fonts/') && url.pathname.endsWith('.ttc')) fontRequests.push(url.pathname); if (['http:', 'https:'].includes(url.protocol) && url.origin !== origin) externalRequests.push(request.url()); });
  const ready = () => page.waitForFunction(() => document.body.dataset.state === 'ready' && document.body.dataset.busy === 'false', null, { timeout: 120000 });
  const latestStats = () => page.evaluate(() => window.__renders.at(-1).stats);
  const download = async id => {
    const event = page.waitForEvent('download'); await page.click(`#${id}`); const file = await event;
    const destination = path.join(output, file.suggestedFilename()); await file.saveAs(destination);
    await page.waitForFunction(() => document.body.dataset.busy === 'false');
    return fs.readFileSync(destination);
  };
  try {
    await page.goto(`${origin}/wasm-preview.html`);
    await page.evaluate(() => {
      window.__compiles = []; window.__renders = []; window.__ticks = 0;
      window.addEventListener('slideshuttle:compiled', e => window.__compiles.push(e.detail));
      window.addEventListener('slideshuttle:rendered', e => window.__renders.push(e.detail));
      setInterval(() => window.__ticks++, 10);
    });
    await page.waitForFunction(() => !document.getElementById('run').disabled);
    assert.equal(await page.locator('#fonts').evaluate(input => input.files.length), 0);
    const start = Date.now(), ticksBefore = await page.evaluate(() => window.__ticks);
    await page.click('#run'); await ready();
    const firstLoadMs = Date.now() - start, responsiveTicks = await page.evaluate(() => window.__ticks) - ticksBefore;
    assert(responsiveTicks > 0, 'main event loop should run while Worker renders');
    assert.equal(await page.locator('#page-label').textContent(), '1 / 3');
    assert.deepEqual(await latestStats(), { compilations: 1, fontBuilds: 1, cacheHits: 0 });
    await page.screenshot({ path: path.join(output, 'preview.png'), fullPage: true });
    const png = await download('png');
    const native = path.join(root, 'examples/output/preview/slide-001.png');
    assert(png.equals(fs.readFileSync(native)), 'browser PNG should match the native demo render');
    await page.click('#next'); await ready(); assert.equal(await page.locator('#page-label').textContent(), '2 / 3');
    await page.click('#next'); await ready(); assert.equal(await page.locator('#page-label').textContent(), '3 / 3');
    assert.equal((await latestStats()).compilations, 1, 'turning pages must not recompile');
    await page.click('#previous'); await ready();
    assert.equal(await page.evaluate(() => window.__renders.at(-1).renderStats.pngCacheHits), 1, 'returning to a page should reuse encoded PNG');
    const pdf = await download('pdf'), pptx = await download('pptx');
    assert.equal(pdf.subarray(0, 5).toString(), '%PDF-'); assert.equal(pptx.subarray(0, 2).toString(), 'PK');

    await page.locator('#source-panel').evaluate(element => element.open = true);
    const original = await page.locator('#xml').inputValue();
    await page.locator('#xml').fill(original.replace('让想法', '把想法'));
    await page.click('#recompile'); await ready();
    assert.equal((await latestStats()).compilations, 2); assert.equal((await latestStats()).fontBuilds, 1);
    await page.locator('#xml').fill('<Deck><Slide id="bad"><Text id="t" style="unknown:yes">Bad</Text></Slide></Deck>');
    await page.click('#recompile'); await page.waitForFunction(() => document.body.dataset.state === 'error' && document.body.dataset.busy === 'false');
    assert(await page.locator('#slide').isHidden()); assert(await page.locator('#pdf').isDisabled()); assert(await page.locator('#recompile').isEnabled());
    await page.locator('#xml').fill(original); await page.click('#recompile'); await ready();
    assert.equal((await latestStats()).fontBuilds, 1, 'recovering from source errors should reuse loaded fonts');

    // A valid but tight box must retain its warning after the page is painted.
    await page.locator('#xml').fill('<Deck font-family="Noto Sans CJK SC"><Slide id="tight"><Text id="label" style="width:49px;height:34px;font-size:24px;line-height:34px">中文</Text></Slide></Deck>');
    await page.click('#recompile'); await ready();
    assert((await page.locator('#diagnostics').textContent()).includes('text_tight'), 'page rendering should preserve compiler warnings');

    const missingFontSource = '<Deck font-family="Missing Preview Font"><Slide id="fallback"><Text id="fallback-title" style="width:900px;height:90px;font-size:32px;font-style:italic">中文字体替代 / preview fallback</Text></Slide></Deck>';
    await page.locator('#xml').fill(missingFontSource); await page.click('#recompile'); await ready();
    assert(await page.locator('#font-notice').isVisible(), 'font substitutions must be visible outside collapsed diagnostics');
    assert.equal(await page.locator('#xml').inputValue(), missingFontSource, 'preview substitution must not rewrite the source editor');
    assert((await page.locator('#diagnostics').textContent()).includes('font_substitution'));
    await page.locator('#resources-panel').evaluate(element => element.open = true);
    await page.locator('#font-fallback').uncheck();
    await page.click('#recompile');
    await page.waitForFunction(() => document.body.dataset.state === 'error' && document.body.dataset.busy === 'false');
    assert((await page.locator('#status').textContent()).includes('font face not supplied'), 'strict mode must report unavailable original fonts');
    assert(await page.locator('#slide').isHidden());

    await page.locator('#fonts').setInputFiles([path.join(root, 'crates/slide-shuttle-core/tests/fonts/DejaVuSans.ttf'), path.join(root, 'crates/slide-shuttle-core/tests/fonts/DejaVuSans-Bold.ttf')]);
    await page.locator('#xml').fill('<Deck font-family="DejaVu Sans"><Slide id="custom"><Text id="custom-title" style="width:900px;height:90px;font-size:32px;font-weight:700">Custom font resource</Text></Slide></Deck>');
    await page.click('#run'); await ready();
    assert(await page.locator('#font-notice').isHidden(), 'provided custom faces should render without substitution');
    await page.locator('#fonts').setInputFiles([]);
    await page.locator('#font-fallback').check();
    await page.locator('#resources-panel').evaluate(element => element.open = false);

    await page.locator('#source').setInputFiles(path.join(root, 'examples/output/SlideShuttle-demo.pptx'));
    await page.click('#run'); await ready();
    assert.equal(await page.locator('#page-label').textContent(), '1 / 3');
    assert(await page.locator('#losses').isVisible()); assert(await page.locator('#pptx').isDisabled());
    assert(await page.locator('#pdf').isEnabled());
    await page.locator('#source-panel').evaluate(element => element.open = false);
    await page.screenshot({ path: path.join(output, 'pptx-preview.png'), fullPage: true });

    await page.click('#run'); await page.click('#cancel');
    await page.waitForFunction(() => document.body.dataset.state === 'cancelled');
    assert(await page.locator('#slide').isHidden());
    await page.waitForFunction(() => !document.getElementById('run').disabled);
    await page.click('#run'); await ready();
    assert.equal(await page.locator('#page-label').textContent(), '1 / 3');

    await page.locator('#source').setInputFiles(emptyTextFixture);
    await page.click('#run'); await ready();
    assert.equal(await page.locator('#page-label').textContent(), '1 / 3');
    assert(!(await page.locator('#diagnostics').textContent()).includes('text_overflow'), 'empty text containers on shapes and zero-height placeholders must not block imported PPTX preview');
    await page.click('#next'); await ready();
    await page.click('#next'); await ready();
    assert.equal(await page.locator('#page-label').textContent(), '3 / 3');

    await page.locator('#source').setInputFiles(themeFixture);
    await page.click('#run'); await ready();
    assert(await page.locator('#losses').isHidden(), 'supported theme colors, alpha and default text styles must be reconstructed, rather than hidden behind generic loss notices');
    assert(await page.locator('#font-notice').isHidden());
    for (let index = 0; index < 3; index++) {
      const png = await download('png');
      assert(png.equals(fs.readFileSync(path.join(themeNative, `slide-${String(index+1).padStart(3, '0')}.png`))), `theme page ${index+1} differs from independently authored explicit-color/default-style XML`);
      fs.writeFileSync(path.join(output, `theme-preview-${index+1}.png`), png);
      if (index < 2) { await page.click('#next'); await ready(); }
    }

    await page.locator('#source').setInputFiles(path.join(root, 'examples/components.xml'));
    await page.locator('#images').setInputFiles(path.join(root, 'examples/palette.png'));
    await page.click('#run'); await ready();
    await page.click('#next'); await ready();
    const imagePage = await download('png');
    assert(imagePage.equals(fs.readFileSync(path.join(root, 'examples/output/components-native/slide-002.png'))), 'image asset aliases and fit modes should render identically in browser and native');

    await page.locator('#folder').setInputFiles(path.join(root, 'examples/project'));
    await page.click('#run'); await ready();
    assert.equal(await page.locator('#page-label').textContent(), '1 / 3');
    assert((await download('png')).equals(fs.readFileSync(path.join(root, 'examples/output/project-native/slide-001.png'))), 'a folder project should render like the native CLI');
    await page.locator('#source-panel').evaluate(element => element.open = true);
    assert.equal(await page.locator('#file option').count(), 5, 'the editor should list the entry, library and every page file');
    await page.locator('#file').selectOption('pages/02-principles.xml');
    const principles = await page.locator('#xml').inputValue();
    assert(principles.startsWith('<Slide id="principles"'), 'switching files should show that file');
    await page.locator('#xml').fill(principles.replace('background="#EFF5F3"', 'background="#FFF7E6"'));
    await page.click('#recompile'); await ready();
    assert.equal(await page.locator('#page-label').textContent(), '1 / 3', 'editing one page file recompiles the whole project');
    await page.locator('#source-panel').evaluate(element => element.open = false);

    assert.equal(fontRequests.length, 2, 'opening another document or replacing the Worker must reuse the built-in font bytes');
    const report = { engineVersion: execFileSync(path.join(root, 'target/debug/slide-shuttle'), ['--version'], { encoding: 'utf8' }).trim(), browser: browser.version(), worker: true, builtinFontsAutomatic: true, builtinFontDownloads: fontRequests.length, sourceFileOnlyPreview: true, fontSubstitutionsVisible: true, strictFontMode: true, customFontsSupported: true, nativePngByteMatch: true, imageAssetNativeByteMatch: true, pageCacheHits: 1, pageNavigationCompileCount: 1, fontBuildsAfterSourceEditAndRecovery: 1, invalidSourceClearsPreview: true, warningsRemainVisible: true, importedPptxPreview: true, emptyTextPptxPreview: true, themeStylePptxPreview: true, themeOracleByteMatchPages: 3, themeImportLosses: 0, importLossesVisible: true, importedPptxReexportGuarded: true, cancelAndReload: true, firstLoadMs, responsiveTicks, pdfBytes: pdf.length, pptxBytes: pptx.length, pageErrors: errors, externalRequests };
    assert.deepEqual(errors, []); assert.deepEqual(externalRequests, []);
    fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally { await context.close(); await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; }).finally(() => python.kill());
