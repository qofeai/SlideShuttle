# SlideShuttle

独立的 Rust 演示文稿编译器。Agent 用接近 HTML/CSS 的静态声明描述页面，原生布局与文字整形引擎生成 PNG、PDF，以及包含原生对象的 PPTX。

当前版本 **0.3.0**，提供原生渲染库、CLI 和运行于 Web Worker 的 WASM 预览器。格式由本项目定义，不读取 Kimi 的 PPTD；核心渲染不依赖浏览器或 Office 服务。**不承诺任意 PPTX 无损往返，也尚未通过真实 PowerPoint / WPS 验收。**

## 开始使用

需要 Rust 1.92+。核心库不扫描系统字体；命令行也要求显式提供字体文件。中文示例使用 Noto Sans CJK SC 的常规、粗体两个字重；没有本机字体时可用 `python3 tools/fetch_fonts.py` 下载到 `examples/fonts/`：

```bash
cargo build -p slide-shuttle-cli

target/debug/slide-shuttle check examples/demo.xml \
  --font /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
  --font /usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc

target/debug/slide-shuttle render examples/demo.xml --out examples/output/preview --pdf \
  --font /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
  --font /usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc

target/debug/slide-shuttle export examples/demo.xml --out examples/output/demo.pptx \
  --font /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
  --font /usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
```

路径按本机字体位置调整。字体也可在源文件中声明为 `<Font family="…" src="fonts/regular.ttf"/>`。声明的字体、图片路径必须留在源文件目录内；`--font` 可显式提供其他位置的字体。只有常规字体时，使用粗体会报缺少字重，不做静默伪粗体。PNG/PDF 使用提供的字形，PDF 嵌入子集字体；**PPTX 目前只引用字体名称，收件端需要安装相同字体**。

输出路径必须不存在。再次生成请选择新文件或目录，避免把尚未检查的外部版本覆盖。

## 工作区

| 包 | 职责 |
|---|---|
| `slide-shuttle-core` | 解析、类型化文档、布局、原生绘制、OOXML、有限导入、三路比较 |
| `slide-shuttle-cli` | 资源加载、命令行、旁车和导出回执；命令 `slide-shuttle` |
| `slide-shuttle-wasm` | `Engine` / `CompiledDeck`，编译后按页渲染，显式注入字体和图片 |

源文件只有一份权威。解析后的文档保留稳定 ID；布局场景是派生结果。PNG 和 PDF 消费同一份已经整形、定位的字形。PPTX 保留文本和对象，Office 会使用自身排版引擎，因此不能把原生预览当成 Office 的像素级保证。原生数据图表的 Office 排版也会与本项目预览不同。

## 语言示例

```xml
<Deck version="1" width="1280px" height="720px" font-family="DejaVu Sans">
  <Font family="DejaVu Sans" src="fonts/DejaVuSans.ttf"/>
  <Slide id="intro" background="#F3F8F6">
    <Box id="columns" style="display:grid;grid-template-columns:2fr 1fr;gap:32px;padding:56px">
      <Text id="title" style="height:150px;font-size:48px;line-height:64px">
        A clear idea.<br/>A native slide.
      </Text>
      <Shape id="accent" style="height:150px;background:#09A88F;border-radius:16px"/>
    </Box>
  </Slide>
</Deck>
```

支持 `Box`、`Group`、`Text` / `Span` / `br`、`Image`、`Shape`、`Path`、`Table`、`Chart`，以及 `Token`、`Style`、`Component` / `Use`。这是一门静态语言，不能执行 JavaScript。完整语法与限制见 [语言说明](docs/LANGUAGE.md)。

## 导入与继续改稿

```bash
slide-shuttle import original.pptx --out imported --font fonts/regular.ttf --font fonts/bold.ttf
slide-shuttle check imported/deck.xml --font fonts/regular.ttf --font fonts/bold.ttf
slide-shuttle inspect original.pptx

# baseline 路径在 export 的 JSON 输出和 receipt 中
slide-shuttle diff examples/demo.xml --base BASELINE.json --against externally-edited.pptx \
  --font fonts/regular.ttf --font fonts/bold.ttf --out review.json
```

导入产生本项目 XML、图片、`losses.json` 和私有的 `.slide-shuttle/base.json`。它重建支持的对象，使用绝对布局；不恢复原来的组件、Grid 或设计意图。导入后检查损失、布局和字体，再处理必要的源文件修改。导入工程再次 export 会自动读取该基线。需要检查新的外部稿时，必须显式提供 `--against`。

比较基线 B、外部稿 E、计划输出 P，分别报告文本、几何、样式、原生结构、增删、重复身份和包部件。外部修改未采纳、真实冲突、未知身份、未重建内容会阻断导出。没有提供 E 时输出 `externalChecked: false`，不能代表已检查其他设备上的副本。

确实需要有损重建时，使用 `--accept decisions.json` 记录具体处理依据：

```json
{
  "diffId": "复制当前 review.json 的 diffId",
  "items": {
    "某个阻断 change 的 id": "按当前任务生成静态新文件，明确舍弃这个动画"
  }
}
```

输入变化后决策失效。它不能绕过格式、资源和布局错误。工具不自动合并或修改原 PPTX；它写新产物，并把处理结果写入回执。详细边界见 [实现与验证状态](docs/STATUS.md)。

基线包含源正文与历史状态，默认留在源目录的 `.slide-shuttle/`，不写进交付 PPTX。完整比较和决策留在私有目录；回执仅记录摘要、路径和接受项 ID。分享工程前自行检查私有目录；删除工程时一起删除旁车。当前没有平台持久化或客户端集成。

## WASM

```bash
rustup target add wasm32-unknown-unknown
cargo build -p slide-shuttle-wasm --target wasm32-unknown-unknown --release
cargo install wasm-bindgen-cli --version 0.2.130 --locked
wasm-bindgen --target web --out-dir examples/output/wasm \
  target/wasm32-unknown-unknown/release/slide_shuttle_wasm.wasm
```

```javascript
import init, { Engine } from './wasm/slide_shuttle_wasm.js';
await init();
const engine = new Engine();
engine.addFont('Noto Sans CJK SC', regularFontBytes);
engine.addFont('Noto Sans CJK SC', boldFontBytes);
engine.addImage('hero', pngBytes); // 对应源文件的 src="hero"
const deck = engine.compile(sourceXml);
const report = JSON.parse(deck.inspect());
if (!report.valid) throw new Error(JSON.stringify(report.diagnostics));
const png = deck.renderPng(0, 1); // 页码从 0 开始；同一编译结果支持多次渲染
const pdf = deck.renderPdf();
const pptx = deck.exportPptx();
deck.free();
engine.free();
```

`CompiledDeck` 持有不可变的资源和布局快照。翻页不重新排版；最多缓存 8 个“页码 + 缩放”PNG，总编码字节不超过 32 MiB。同一 Engine 修改源文件时复用字体系统；修改字体/图片后重建编译器，旧文档仍然有效，需主动 `.free()`。原有 source-based 方法保留，复用最近一次相同源码的编译结果。缓存上限不是 WASM 总内存上限。

本地 [WASM 预览器](examples/wasm-preview.html) 支持 XML / 有限 PPTX 导入、翻页、源文件修改、诊断与损失查看、PNG/PDF 下载和自创稿 PPTX 导出。构建 web bindings 并下载内置字体后启动：

```bash
python3 tools/fetch_fonts.py
python3 -m http.server 8088 --bind 127.0.0.1 --directory examples
```

访问 `http://localhost:8088/wasm-preview.html`，直接选择文稿并点“打开并预览”，无需手动提供字体。不选文稿时加载中文示例；也可以选择 `examples/components.xml` 和图片 `examples/palette.png`。预览应用随附 Noto CJK 常规、粗体，首次从同源加载约 38 MiB，后续打开复用字体字节；发布页面时须保留 [fonts 资产与许可](examples/fonts/README.md)。编译与渲染在 Worker 内运行，取消会终止 Worker，重新打开会加载新实例。文件在本机处理。

预览器默认使用内置字体替代未提供的字体/样式，并显示提示；不修改源编辑器中的字体声明。可在“字体与图片”中补充原字体，或关闭替代恢复严格检查。字体替代会影响文字宽度与换行，不保证原稿外观。核心/CLI 与默认 WASM API 仍严格检查字体；宿主可显式用 `Engine.setFontFallback('Noto Sans CJK SC')` 开启替代，用 `undefined` 关闭。

PPTX 导入是支持对象的重建预览；页面保留损失提示，导入稿的 PPTX 重建需走 CLI 的差异与损失决策。WASM 底层 API 的基线持久化和决策仍由宿主负责。Chromium 已实际测试，Firefox/Safari 和大文稿压测尚未完成。API、文字测量和完整复现步骤见 [渲染说明](docs/RENDERING-0.2.0.md)。

## 验证

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p slide-shuttle-core --target wasm32-unknown-unknown
node --test tools/*.test.mjs
python3 -m unittest discover -s benchmarks -p 'test_*.py'
```

独立文件校验脚本见 [verify_outputs.py](tools/verify_outputs.py)，WASM 运行冒烟脚本见 [wasm_smoke.cjs](tools/wasm_smoke.cjs)，真实浏览器检查见 [browser_smoke.cjs](tools/browser_smoke.cjs)。CI 已配置实际 Chromium + Worker + WASM 检查；本地执行结果与远程 CI 运行分开记录。测试字体与许可在 `crates/slide-shuttle-core/tests/fonts`。依赖来源、已验证事项和剩余验收项记录在 [STATUS.md](docs/STATUS.md)。

公开任务的实际生成、一次修复、独立 PPTX 渲染和指标结果见 [SlidesBench / SlidesGen 首轮实测](docs/BENCHMARK-2026-11-02.md)，输入准备与评测脚本位于 [benchmarks](benchmarks/README.md)。

0.3.0 新增主题色、透明度和默认文字样式继承的导入解析，范围与独立渲染结果见 [OOXML 导入说明](docs/OOXML-IMPORT-0.3.0.md)。

## 创作 Skill

[slideshuttle Skill](skills/slideshuttle/SKILL.md) 把网页编排思路落实为内容组织、静态 XML、原生渲染、看图改稿和交付流程。包内有设计/语法/复核指南、五页构图示例和自动构建脚本；源码目录可复制给支持 SKILL.md 的 Agent 使用。

```bash
python3 skills/slideshuttle/scripts/build.py --doctor
python3 skills/slideshuttle/scripts/build.py /path/to/deck.xml --out /path/to/output/v1
```

构建会检查并生成 PPTX、PDF、逐页 PNG 与本地静态总览，自动提供项目自带的中文常规/粗体；可用 --repo / --cli / --font 指定其他运行环境。视觉复核仍由创作 Agent 查看实际渲染完成。已有 PPTX 的损失/基线保护继续由 CLI 执行。
