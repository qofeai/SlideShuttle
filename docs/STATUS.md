# 实现与验证状态

日期：2026-11-03。仓库与产品名：**SlideShuttle**。包版本：**0.3.0**，源语言仍为 v1。原生 XML 可直接预览和按点击步骤放映，另可输出 PDF、原生对象 PPTX。真实 PowerPoint/WPS、任意文件往返、连续动画与正式客户端接入仍有明确缺口。

## 2026-11-03 原生展示与性能审查

新增 core/WASM RGBA 输出、点击步骤接口与浏览器按步放映；CLI 改为临时目录逐张生成，修复单页资源加载及组/子元素动画组合。100 页、300 张 PNG 的 CLI 峰值 RSS 从 162.14 MiB 降到 26.89 MiB，300 张 PNG 逐字节一致。release 文字页 RGBA 相比同版未缓存 PNG 约快 2.66 倍；这些是原生 CPU 测量，不是移动设备 WASM 帧率。

Rust 1.92.0 / macOS ARM64 实际运行 workspace 测试、严格 Clippy、WASM 检查与构建、Node WASM、Chromium 156 Worker 放映回归。连续时间采样、真实手机、Safari/Firefox 和 100 页 WASM 内存尚未验证。完整范围、实测数据、测试入口与后续优先级见 [审查报告](AUDIT-2026-11-03.md)。以下保留既有能力表及 2026-11-02 历史验证记录；历史测试数量不代表当前 workspace 总数。

## 已实现

| 范围 | 实现 |
|---|---|
| 语言 | 严格 XML、静态样式、Token、组件展开、稳定 ID、行列来源映射 |
| 布局 | Taffy block / Flex / 受限 Grid，显式和绝对布局，关闭取整 |
| 文字 | 显式字体、真实字重、COSMIC Text 整形与换行、缺字失败；可显式启用带诊断的字体替代 |
| 预览 | tiny-skia 原生 PNG；krilla 原生 PDF；编译文档持有资源快照，按页缓存最多 8 项 / 32 MiB 编码 PNG |
| 绘图 | 基本形状、圆角、M/L/C/Z 路径、PNG/JPEG、两色线性渐变 |
| PPTX | 文本、形状、图片、一层组、表格、原生柱状/折线与嵌入 XLSX |
| 导入 | 受限 OOXML 重建为绝对布局、占位符几何回退、图片资产、损失清单 |
| 改稿 | B/E/P 比较、对象歧义、派生位移、删除/编辑冲突、原生结构/整包清单、导入遗漏阻断 |
| 决策 | 当前 diff 摘要绑定逐项理由，输入变化后失效，私有旁车与摘要回执 |
| CLI | measure / check / render / export / import / inspect / diff，显式资源、写新文件 |
| WASM | Engine / CompiledDeck、复用字体编译、真实文字测量、资源字节注入、PNG/PDF/PPTX 及有限导入 |
| 原生放映 | stepCount、PNG/RGBA 点击步骤、Worker/Canvas 全屏与窄屏展示、当前步骤截图；暂不播放连续动画与页面过渡 |
| 浏览器 | 内置中文常规/粗体、缺失字体替代提示、可补充原字体；module Worker 编译/渲染、翻页、源码重编译、资源加载、错误恢复、终止取消、导入损失展示 |
| 示例 | 三页中文设计稿；三页列表/图片/表格/折线图样例；创作 Agent 操作说明 |

## 已执行证据

环境：Linux x86_64，Rust 1.98.1；依赖版本由 Cargo.lock 固定。源码声明最低 Rust 1.92；依赖的声明下限已核对，但没有单独在 1.92 工具链上执行。

- `cargo test --workspace`：51 项集成测试通过，其中核心 26 项、编译文档 11 项、导入样式 9 项、CLI 5 项；不依赖数据库、浏览器、Office 或系统字体。测试字体随仓库携带并保留许可。覆盖默认严格检查、字体替代及诊断、Span 继承、实际导出字体、缺失斜体不伪造、缺字仍失败，以及空文本/空文本容器的导入回归。
- `cargo fmt --all --check` 与严格 Clippy；WASM 的 core 目标检查与实际 release 构建。
- 三页示例 PNG 已逐页目视检查；同份场景生成 PDF。
- 独立工具 python-pptx 1.0.2 读出 3 页、34 个文本对象、1 个原生组、1 个图表和 1 个表格；这证明第三方库能读其结构，不能证明 Office 已验证可编辑。
- openpyxl 3.1.5 能读取图表嵌入的 XLSX，检查末个数据点为 82。
- PyMuPDF 2.0 能提取并搜索三页中文；PDF 第一页另行栅格化并目视核对。
- Node 实际加载 release WASM：完成 compile、check、measureText、三页 PNG、PDF、PPTX 和导入。三张 PNG 与本机原生 PNG **逐字节一致**；验证了 Engine clear/free 后旧文档仍可渲染未缓存缩放，翻页共享同一编译结果。这只是当前构建和样本的结果，不是所有平台的无条件保证。
- Chromium 156.0.8078.4 实际运行浏览器页面、module Worker 和 WASM；中文示例、图片别名/缩放页与原生 PNG 逐字节一致。翻页不增加编译次数，源码修改和错误恢复保持一次字体系统构建，测试了下载、取消/重载、导入损失提示和导入稿重建限制；没有页面异常或外部网络请求。报告：`examples/output/browser-v2/report.json`。4 项 Worker 通信测试通过，覆盖过期响应、取消、初始化中取消与 Worker 错误。
- 内置字体版本的真实浏览器复测：不上传字体，中文示例与只选择 PPTX 均可打开；跨文稿/Worker 重建只下载两份字体一次，替代提示、关闭替代后严格失败、补充 DejaVu 字体均通过。字体缓存保留原字节，向 Worker 传输独立副本；3 项加载器测试覆盖传输后再用、下载失败重试和取消后重试。
- 修复空文本虚构一行 32px 导致零高度对象溢出的错误。真正的空文本测量为 0×0、0 行，作者明确输入的空白/换行仍保留行距；PPTX 中带空 txBody 的线、椭圆、路径按原几何导入，纯空文字占位符保留身份。回归确认形状/线条/路径的 PNG 不因空 txBody 改变，真实文字的零高度溢出仍阻止输出。浏览器使用三页、额外包含 12 个空对象的 PPTX 验证导入翻页，报告字段为 `emptyTextPptxPreview`。
- CLI 真正运行外部改字 + timing 新增、导入后重建阻断、具体决策接受、源变化后旧决策失效，以及文件已存在时拒绝覆盖。
- 同份自创三页稿与自身基线比较为 0 项变化；生成的 PPTX 再导入后可以通过 check 并重新原生渲染。导入明确报告组层级展开；三页中第 3 页像素一致，第 1、2 页存在像素差异，保留 `examples/output/roundtrip-verification.json`，不将此结果称为无损往返或 Office 保存验证。
- 新增 [公开 benchmark 实测](BENCHMARK-2026-11-02.md)：SlidesBench 12 条无外部图片任务，首轮 7/12，一次编译诊断修复后 12/12；通过独立 LibreOffice 渲染运行适配的 SlidesGen 计算指标。发现一页标签换行问题和空母版导致的深层编辑能力缺口。另检查了上游 31 页真实 PPTX。评测脚本新增四项独立测试。
- 0.2.0 使用该轮修复后的 12 页 XML 原样回归，不重新生成内容。修复内在宽度标签的 OOXML 换行策略；LibreOffice 26.2.6.3 渲染后逐词诊断未发现四字符及以上单词的缺失或拆词，五个已知标签恢复单行。报告：`examples/output/rendering-v2/report.json`。这不代表全视觉验收或重新获得官方 benchmark 分数。
- 新增三页组件稿也通过独立 LibreOffice 渲染。python-pptx 读出 3 张原生图片、1 张表格和 1 个折线图；openpyxl 确认工作簿保留负数 -8。原生和独立渲染已目视核对，图表的坐标轴、图例外观因渲染器不同而不同。报告：`examples/output/components-office/report.json`。

验证脚本与 API 复现见 [RENDERING.md](RENDERING.md)。输出文件在 `examples/output/`，忽略入 Git。GitHub Actions 在每个 PR 与 main 上运行 Rust/WASM 检查和真实 Chromium 任务，已实际通过。示例布局在 `examples/demo.xml` 与 `examples/components.xml`，图表数值仅作演示。

## 尚未通过的门槛

| 项目 | 状态与下一步 |
|---|---|
| Windows PowerPoint 实际渲染/编辑 | 未执行；提供 `tools/office_probe.ps1` 草案，需真实 Windows Office 跑无编辑保存、改字、移动、复制、删除并保留原文件、版本、字体、截图和文字边界 |
| WPS | 未执行，独立验收；不能用第三方 XML 库或 LibreOffice 替代其证据 |
| Office 保存噪声 | 规范化忽略 dirty、关系和未知部件保守摘要；缺真实 Office 无编辑保存样本，当前会有保守误报 |
| PPTX 字体嵌入 | 未实现；仅命名字体引用，收件人需相同字体；PDF 已嵌入子集 |
| Office 折行与图表外观 | 已修复并用 LibreOffice 回归紧标签折行；其他渲染器仍自行排版，不给出像素一致承诺；需要 PowerPoint/WPS 字体/行距探针 |
| 导入覆盖 | 支持主题色/字体和常用文字属性级联；完整主题效果与复杂段落仍有限；组展开；不恢复 Flex/Grid/组件；复杂表格、SmartArt、动画、备注等有损或未建模 |
| 原包 patch / 自动合并 | 未实现；只比较并生成新文件，不向任意外部 PPTX 打补丁 |
| 几何回写 | 有派生位移分类；最小区域冻结、自动碰撞分析与修复仍未实现 |
| CJK 完整排版 | 已验证示例混排、粗体、折行与中文搜索；竖排、注音、复杂禁则等未专项验收 |
| 浏览器兼容 / 性能 | Chromium + Worker 已实测、按页编码缓存有上限；Firefox/Safari、100 页长文档、总内存峰值、字体变更时多份快照存活的压测未完成 |
| Master/Layout | 尚无真实母版内容继承；现有输出空母版不能作为深层编辑能力证明 |
| Agent 创作收益 | 已跑 12 条公开任务的生成/一次诊断修复；四组同模型/素材/预算实验未执行，不能声称已证明比现成生成器更美观 |
| 平台接入 | 尚未接入任何宿主平台；旁车持久化、删除策略与发布白名单需要宿主侧契约 |

其中 PowerShell 探针还没有在 PowerPoint 环境运行，属于可复核的实验起点，不是已完成的测试结果。仍需补组合/跨页等操作和组内对象边界的专项采集。

## 资源与数据边界

核心不调用浏览器排版、不抓取网络、不扫描系统字体。CLI 读取显式字体与工程内图片；WASM 由宿主提供字节。PPTX 导入不获取外部关系。ZIP/XML/图片/字体和渲染表面有预算。`fuzz/` 提供源码全流程（解析→布局→PNG/PPTX）、原始 PPTX 包和结构化 slide XML 三个 libFuzzer 目标，首轮各运行 5 分钟（约 5.6 万、40 万、2.8 万次）未发现崩溃、超时或内存超限，此后每周定时运行；这不等同于全面的恶意文档安全审计。

导出保留新原生基线与源快照。导入工程另外标识为 imported，重建损失必须进入决策；原生检查器报告的“导入时会展开 Group”不会让一个未改动的自创 Group 工程误报丢失。外部文件新增未建模内容与真正未重建内容仍阻断。对象原生结构摘要提供保守兜底，不把原生 ID/name 当成绝对可靠身份。

私有比较报告包含历史正文，保存在 `.slide-shuttle/`。交付 PPTX 不嵌入源码或历史；旁边的回执仅含摘要、路径与接受项 ID。删除工程时一并删除旁车；当前没有后台生命周期服务。

## 基础依赖

核心设计与格式为独立实现，未使用 Kimi 的私有格式、前端、WASM、模板或代码。采用的通用开源库包括 [Taffy](https://github.com/DioxusLabs/taffy)、[COSMIC Text](https://github.com/pop-os/cosmic-text)、[tiny-skia](https://github.com/linebender/tiny-skia)、[krilla](https://github.com/LaurenzV/krilla)、[ttf-parser](https://github.com/harfbuzz/ttf-parser)、[image](https://github.com/image-rs/image) 和 [zip](https://github.com/zip-rs/zip2)。具体许可随上游包提供；本项目源码 MIT OR Apache-2.0，测试字体为独立许可。浏览器内置 Noto CJK 的 SIL OFL 1.1 许可、来源与字节摘要保存在 [examples/fonts](../examples/fonts/README.md)。

## 0.3.0 导入解析

已补主题关系、颜色映射、常用变换、填充/边线/文字和图片透明度、默认文字继承。主题三页样例的浏览器 PNG 与显式样式参考稿逐字节一致；LibreOffice 的 9 个色块采样最大通道差 2/255，保留差异记录。范围、规则来源与未覆盖事项见 [OOXML-IMPORT.md](OOXML-IMPORT.md)。历史 benchmark 结果仍属于其记录的旧版本。
