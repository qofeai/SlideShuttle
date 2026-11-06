# QPPT 原生文稿

`.qppt` 是 SlideShuttle 的原生交付文件：一个 ZIP 容器，保存多文件源工程、字体和引用资源。客户端在内存中读取并直接编译、渲染，不经过 PPTX，不需要落盘解压。清单没有 `version` 字段。

## 工程与封装

```text
presentation.qppt
  manifest.json
  deck.xml
  design.xml
  pages/cover.xml
  pages/content.xml
  media/hero.png
  fonts/regular.ttf
```

`manifest.json` 的必要内容只有格式和入口：

```json
{"format":"qppt","entry":"deck.xml"}
```

入口固定为包根目录的 `deck.xml`。它声明尺寸、默认字体、共享库和页面顺序，每页通过 `<Slide src="pages/cover.xml"/>` 引用。即使只有一页，也必须放在独立页面文件中；CLI 和 WASM 工程入口不接受内联 Slide 的单 XML 文稿。原始 XML 解析函数用于内部片段处理和测试。

字体可以通过 XML `<Font family="…" src="fonts/regular.ttf"/>` 声明，或通过 `pack --font FILE` 提供。`pack` 默认根据实际排版用到的 face、字符和字形创建子集：包括回退字体、测量过程、组件、表格、图表生成标签、列表标记及 OpenType 排版依赖。每个 face 成为独立 TTF/OTF，按内容摘要去重并记录在清单既有的 `fonts` 数组中；包内 XML 的 `<Font>` 文件声明被移除，字体样式、原始创作目录和其他源内容不变。

打包前后校验实际使用字形的轮廓和单位，再重新编译，比较字形映射、位置、字宽、行数与布局。无法保真时拒绝输出，可显式选择 `--full-fonts` 保留完整字体，不会静默回退成一个大包。HarfBuzz 子集库仅进入 CLI，不增加 WASM 或移动阅读器体积。格式没有新增字段，旧阅读器仍能打开。

子集足够离线显示当前文稿；解包后新增字符、字重或语言时，需要在创作环境用原始完整字体替换资源再打包。不要把子集当作完整字库。需要不依赖创作环境的离线自由编辑时，从提供完整字体的工程使用 `pack --full-fonts`；这种包保留原字体文件和声明。该选项不会从现有子集还原已删除的字形。QPPT 打开不下载默认 CJK 字体，也不依赖收件端系统字体。

图片、SVG 图标、页面背景按实际引用收集。未引用文件、旁车基线、历史记录和构建日志不会自动进入包。源工程文件仍是唯一内容权威，不另外保存必须同步的编译场景或截图缓存。

## CLI

```bash
# 创作目录内有 deck.xml，或直接传一个引用外部页面的入口 XML
slide-shuttle pack ./project --out presentation.qppt \
  --font fonts/NotoSansCJK-Regular.ttc --font fonts/NotoSansCJK-Bold.ttc

# 文件自带资源，后续无需再次传 --font
slide-shuttle check presentation.qppt
slide-shuttle render presentation.qppt --page cover --steps --out preview
slide-shuttle export presentation.qppt --out presentation.pptx
slide-shuttle inspect presentation.qppt

# 还原为可继续编辑的多文件工程
slide-shuttle unpack presentation.qppt --out editable-project
slide-shuttle check editable-project

# 仅在明确需要完整字库时使用（文件通常显著变大）
slide-shuttle pack ./project --out offline-editing.qppt --full-fonts --font fonts/regular.ttf
```

输出路径必须不存在。`pack` 在写出文件前完成严格的字体、资源和布局检查。`unpack` 保留清单与嵌入资源；其产物可直接重新打包。ZIP 输出顺序和时间戳固定，相同工程和资源产生相同的包字节。

## WASM

```javascript
import init, { Engine } from './slide_shuttle_wasm.js';
await init();
const engine = new Engine();
let deck;
try {
  deck = engine.openQppt(new Uint8Array(await file.arrayBuffer()));
  const report = JSON.parse(deck.inspect());
  if (!report.valid) throw new Error(JSON.stringify(report.diagnostics));
  const rgba = deck.renderRgbaStep(0, 0, 1);
  const png = deck.renderPng(0, 1);
} finally {
  deck?.free();
  engine.free();
}
```

需要源码编辑器时，`engine.importQppt(bytes)` 加载资源并返回 JSON `{ entry, files, manifest }`；修改文件后调用 `engine.compileProject(entry, JSON.stringify(files))`，复用当前资源。`openQppt` / `importQppt` 替换 Engine 当前资源集，旧 CompiledDeck 继续持有自己的不可变快照。QPPT 资源字节移动到渲染资源集中，通常不保留额外的解包字体副本。

工程开发仍可用 `addFont` / `addImage` 注入资源后调用 `compileProject`。WASM 不再暴露单源码 `compile/check/renderPng/renderPdf/exportPptx` 方法；渲染和导出统一使用编译文档句柄。

打开 QPPT 时直接编译已解析的文稿，不再把全部 XML 序列化成 JSON 后再次解析。字体数据库与渲染资源共享同一字节分配；打包所需的字体使用统计默认关闭，不给阅读器增加集合收集开销。`openQppt` 仍同步完成全稿排版，宿主应在 Worker / 后台执行。移动端的文件传输与引擎调度是宿主责任，单纯更新库不能消除 RN/WebView 的串行 Base64 桥接成本。

## 导入稿

PPTX 导入工程可打包用于查看。公开清单携带 `imported: true` 与可选 `losses`，私有基线留在原工程。包与解包目录都保留导入身份。WASM 允许 PNG/PDF 与放映，阻止直接重建导入稿 PPTX；CLI 重建必须提供原私有基线并完成现有差异/损失决策。封装不会消除已有导入限制，也不代表任意 PPTX 无损往返。

## 读取约束

容器、展开内容各最多 256 MiB，最多 4096 项；源文件每份最多 4 MiB、项目源文本共 64 MiB，单个字体最多 64 MiB，媒体最多 32 MiB，清单最多 1 MiB。沿用 XML、图片、字体和渲染器自身的预算。

路径使用包内相对路径，拒绝越界、绝对路径、反斜杠、驱动器前缀、符号链接、重复路径与文件/目录冲突。CLI 与 WASM 共用 core 的包解析器；客户端无需另写一套 ZIP 与路径语义。
