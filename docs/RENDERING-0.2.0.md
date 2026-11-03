# SlideShuttle 0.2.0 渲染使用与验证

日期：2026-11-02。源语言为独立的 SlideShuttle Language v1，不兼容 Kimi PPTD。0.2.0 的交付重点是可反复使用的渲染库与本地浏览器预览器。

## 编译与渲染的边界

```text
XML + 显式字体/图片
  → 严格解析与组件展开
  → Taffy 布局 + COSMIC Text 字体整形
  → CompiledDocument / CompiledDeck：场景、定位字形、资源快照
     ├─ 按页 PNG：tiny-skia
     ├─ 全稿 PDF：krilla，嵌入子集字体
     └─ 原生对象 PPTX：文本、形状、图片、表格、图表等
```

PNG/PDF 使用相同的已定位字形，不借用 HTML 截图或 Office 服务。PPTX 交由收件端重新排版，文字折行、图表坐标轴/图例可能不同，不能承诺与原生预览像素一致。

## 先运行本地预览器

在仓库根目录构建：

```bash
cargo build -p slide-shuttle-cli --locked
rustup target add wasm32-unknown-unknown
cargo build -p slide-shuttle-wasm --target wasm32-unknown-unknown --release --locked
cargo install wasm-bindgen-cli --version 0.2.130 --locked
wasm-bindgen --target web --out-dir examples/output/wasm \
  target/wasm32-unknown-unknown/release/slide_shuttle_wasm.wasm
python3 -m http.server 8088 --bind 127.0.0.1 --directory examples
```

访问 `http://localhost:8088/wasm-preview.html`，直接选择文稿并点“打开并预览”。不选文稿时打开内置三页 XML 示例，无需上传字体。应用在 `examples/fonts` 随附 Noto CJK 的常规和粗体，首次从同源加载约 38 MiB，后续打开复用保留的字体字节。该目录需要随页面发布，来源、摘要和 SIL OFL 1.1 许可也在目录内。

“字体与图片”中可以补充 `.ttf` / `.otf` / `.ttc` 原字体。预览器默认替代未提供的字体/样式，并在预览旁显示提示；需要严格检查时关闭“缺失字体使用内置字体替代”。不会合成粗体或斜体，找不到斜体时可能显示常规字形并报告。缺字仍然失败，字体替代也可能造成溢出或换行差异。

- 浏览器支持 XML 和有限 PPTX 导入、逐页预览、PNG/PDF 下载；自创 XML 稿可导出 PPTX。
- 源码面板可改 XML 并重新编译。资源不变时复用字体系统。
- `examples/components.xml` 展示自动占位的列表符号、图片、表格和折线图；选该文稿时，在“附加图片资源”中同时选 `examples/palette.png`。该图片为项目自制测试资产。
- 当前文件选择器使用文件名作为图片键；可通过 Asset 将键映射为源中的别名。含子目录资源的工程需要宿主保留相对键名，或用 CLI 加载。
- PPTX 导入会展示损失，默认可用内置字体预览；可以补充原字体以减小差异。导入稿可以下载预览 PNG/PDF；PPTX 重建需要用 CLI 检查基线、外部差异和损失。

解析、字体整形、排版、图片绘制和导出在 module Worker 中执行。UI 使用请求 ID 与文档 revision 丢弃过期结果；取消会终止 Worker 并拒绝未完成请求，重新打开会使用新实例。文件不会发送到外部服务。无需跨源隔离或 SharedArrayBuffer。

## Rust API

```rust
use slide_shuttle_core::{Compiler, FontResource, Resources};

let mut resources = Resources::default();
resources.fonts.push(FontResource {
    family: "Noto Sans CJK SC".into(),
    bytes: std::fs::read("fonts/NotoSansCJK-Regular.ttc")?,
});
let mut compiler = Compiler::new(resources)?;
let document = compiler.compile(&source_xml)?;
println!("{}", document.inspect());
// 有 error 的场景可查看诊断；render/export 会拒绝。
let png = document.render_png(0, 1.0)?;
let pdf = document.render_pdf()?;
let pptx = document.export_pptx()?;
```

`Compiler` 为同一资源集持有可复用字体系统。`compile()` 成功表示文档已生成，不表示没有布局 error，应检查 `scene().has_errors()` 或 `inspect().valid`。语法、字体等错误直接返回 Err。`CompiledDocument` 的资源快照与编译器独立，编译器释放后仍可使用。

核心不读取 Font/Asset 路径；调用方必须注入实际字节。CLI 负责读文件。WASM 的 Image 可以直接用注入键，也可以用 Asset 别名映射到注入的路径键；相同名称的直接键优先。

核心默认严格匹配已注入的字形。宿主可以通过 `Compiler::set_fallback_font(Some("Noto Sans CJK SC".into()))` 明确开启替代，传 None 恢复严格模式。编译阶段在类型化叶节点/Span 上解析实际字体，并产生 `font_substitution` warning；预览与 PPTX 都使用实际选中的字形名称，调用方原 XML 不变。文字测量仍按请求字体严格执行。

## WASM API 与生命周期

```javascript
import init, { Engine } from './output/wasm/slide_shuttle_wasm.js';
await init();
const engine = new Engine();
engine.addFont('Noto Sans CJK SC', regularBytes);
engine.addFont('Noto Sans CJK SC', boldBytes);
engine.addImage('palette.png', paletteBytes);

let deck;
try {
  deck = engine.compile(xml);
  const report = JSON.parse(deck.inspect());
  if (!report.valid) throw new Error(JSON.stringify(report.diagnostics));
  console.log(deck.pageCount, deck.width, deck.height);
  const png = deck.renderPng(0, 1); // Uint8Array，页码从 0 开始
  const second = deck.renderPng(1, 1);
  const pdf = deck.renderPdf();
  const pptx = deck.exportPptx();
  console.log(JSON.parse(engine.stats()), JSON.parse(deck.stats()));
} finally {
  deck?.free();
  engine.free();
}
```

`Engine.check(source)`、`renderPng(source, page, scale)`、`renderPdf(source)` 与 `exportPptx(source)` 仍可使用，复用最近一次完全相同源码的编译文档。多页 UI 更适合显式持有 `CompiledDeck`。

WASM 的 `engine.setFontFallback('Noto Sans CJK SC')` 开启带诊断的替代，传 `undefined` 关闭；默认关闭。需先提供替代字体的常规字形。更改策略会清除当前源码编译缓存，旧文档不变；`clear()` 恢复严格策略。浏览器示例显式启用这项策略，并在 UI 提示替代，不将它当作无损转换。

`Engine.stats()` 的 `compilations` 统计生成的编译文档，`fontBuilds` 统计字体系统构建，`cacheHits` 统计相同源码命中。`CompiledDeck.stats()` 的 `pngRenders` / `pngCacheHits` 统计 PNG 绘制和缓存命中；`cachedPages` 是“页码 + 缩放”条目数，`cachedBytes` 为缓存的编码字节数。

每个文档采用 LRU，最多 8 项、32 MiB 编码 PNG；不同缩放是不同条目。PDF/PPTX 复用场景但每次生成字节，不使用这份 PNG 缓存。资源集上限 256 MiB；字体文件最多 64 MiB，单个显式图片最多 32 MiB，图片解码和渲染表面另有预算。

`addFont`、`addImage`、`importPptx`、`clear` 会使 Engine 的当前编译器/源码缓存失效。旧 CompiledDeck 仍持有原资源，clear 或 Engine.free 后也可继续渲染，包括未缓存的页码/缩放。用完后须主动 free。上述预算按资源集、文档分别计算；同时保留很多旧文档会增加总内存，32 MiB 缓存并非整个 WASM 的内存上限。Worker 终止才会释放整个实例。

## 给 Agent 的真实文字测量

```bash
target/debug/slide-shuttle measure --text '•' \
  --style '{"family":"Noto Sans CJK SC","size":24,"lineHeight":34}' \
  --nowrap --font /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc
```

当前字体下返回 `width:24`、`height:34`、`lineCount:1`、`glyphCount:1`。首轮公开任务里五页给列表符号分配的空间不足；真实测量能在渲染前暴露这类估算错误。更直接的做法是参考 `components.xml`，让符号用内在宽度，正文使用剩余 Flex 空间。

```javascript
const measured = JSON.parse(engine.measureText(
  '中文 mixed text',
  JSON.stringify({ family: 'Noto Sans CJK SC', size: 24, lineHeight: 36, weight: 400 }),
  320, // 可选宽度，单位 px；传 undefined 表示不限制
  true // 是否软折行
));
```

Rust 对应 `Compiler::measure_text(text, &TextStyle, width, wrap)`。样式 JSON 使用 camelCase，接受 `family`、`size`、`lineHeight`、`weight`、`italic`、`align`、`color`；color 为 `[R,G,B,A]`。未知字段拒绝。支持实际 400/700 字重，不合成粗体；宽度限 `(0,16384]`，输入文本最多 64 KiB。返回宽、高、行数和定位字形数，不自动判断内容是否合适。

## 实际执行过的验证

| 验证 | 本机结果 |
|---|---|
| Rust 集成测试 | 45 项通过，包含资源快照、缓存淘汰、测量、导出换行、显式字体替代、空文本/PPTX 空文本容器和 CLI |
| Worker 请求协议与字体加载 | 7 项通过：过期结果、取消、初始化时取消、进程错误；传输后复用字体、下载失败重试和下载取消后重试 |
| 评测工具 | 4 项 Python 测试通过 |
| fmt / 严格 Clippy / WASM 目标检查 | 通过；release WASM 已实际构建 |
| Node 加载真实 release WASM | 三页 PNG 与原生输出逐字节一致；PDF/PPTX/导入、测量、编译复用和 Engine 释放后的文档均通过 |
| Chromium 156.0.8078.4 + Worker | 不上传字体即可打开示例/PPTX；额外带 12 个空文本容器/零高度对象的三页 PPTX 导入翻页通过；替代提示、关闭替代严格失败、自定义字体通过；中文页/图片页与原生 PNG 逐字节一致；翻页、缓存、修改/错误恢复、警告、下载与取消重载通过 |
| 冻结的 12 页公开任务 XML | 0.2.0 导出后经 LibreOffice 26.2.6.3 独立渲染；已知五个标签拆词修复，未发现四字符及以上单词缺失/拆词 |
| 三页组件稿的独立 PPTX 渲染 | 中文、三种图片 fit、表格和含负数的折线图已核对；原生图片/表格/图表及嵌入工作簿结构可独立读取 |

机器结果与截图在 `examples/output/browser-v2/`、`examples/output/rendering-v2/`、`examples/output/components-office/`。浏览器报告没有页面异常或外部网络请求。首屏耗时约 1.3 秒仅为当前机器和三页样本的观察，不能作为性能承诺。

浏览器测试复现需要先生成原生参考文件，输出目录应不存在：

```bash
target/debug/slide-shuttle render examples/demo.xml --out examples/output/preview --pdf --font /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc --font /usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
target/debug/slide-shuttle render examples/components.xml --out examples/output/components-native --pdf --font /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc --font /usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
target/debug/slide-shuttle export examples/demo.xml --out examples/output/SlideShuttle-demo.pptx --font /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc --font /usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
npm ci --prefix tools/browser
tools/browser/node_modules/.bin/playwright install --with-deps chromium
node tools/browser_smoke.cjs
```

CI 已配置同一套真实浏览器流程，并上传报告与截图；尚未推送/运行远程 CI。完整公开任务回归使用 `tools/render_regression.py`、此前冻结源码和已运行的 LibreOffice 容器，参见 [benchmark 记录](BENCHMARK-2026-11-02.md)；缓存任务不包含在源码仓库中。

空文本回归由 `tools/create_empty_text_fixture.py` 在本项目三页 demo 上加入带空 txBody 的线条、矩形、椭圆和零高度纯文字占位符。浏览器脚本自动生成 `examples/output/browser-v2/empty-text-regression.pptx` 并逐页打开；不读取用户的实际文稿。核心回归还比较添加空 txBody 前后的线条/形状/路径像素，并验证可见文字的真正溢出仍然报错。

## 剩余边界

当前没有真实 PowerPoint/WPS 结果，PPTX 字体嵌入、Firefox/Safari、100 页文稿内存压力测试仍未完成。没有真实 Master/Layout 继承；复杂 PPTX 导入不能忠实复现。CJK 已验证横排、常见混排、真实粗体和 PDF 搜索；竖排、注音与完整禁则未专项验收。Agent 的等预算审美收益实验未执行。本轮交付没有扩展到 Word/Excel 预览或 QOFE 各客户端接入。

主题色、透明度和默认文字样式的后续解析已进入 0.3.0，见 [导入说明](OOXML-IMPORT-0.3.0.md)。本页的编译/渲染 API 保持适用。
