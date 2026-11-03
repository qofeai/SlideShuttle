---
name: slideshuttle
description: "用 SlideShuttle 的静态 XML、Grid/Flex 和原生渲染生成或改排精致的 PPT/PPTX 演示稿。适用于把材料变成演示、优化页面层级和布局、生成源稿及 PNG/PDF/PPTX，或明确要求 SlideShuttle 的任务。"
---

# 用 SlideShuttle 设计演示稿

把网页设计能力用在信息层级、关系、节奏与组合上；用 SlideShuttle Language v1 的静态 XML 表达，交给原生引擎布局、文字整形和渲染。目标是让观众更容易理解内容，并交付能继续修改的文本、形状、表格与数据图表。

## 先确定内容和设计方向

沿用用户的主题、事实、品牌、页数、语言与交付格式。缺少会影响结论的事实时集中询问；受众、风格等非关键偏好可以做明确的合理假设，继续推进。

给每页确定论点、支撑证据、主要视觉表达和它与前后页的关系。事实、数字、引文保留来源；推测和演示数据明确标识。把长文重组为叙事与证据，不逐段搬到幻灯片上。

先选一套适合材料的文字层级、颜色、间距和构图语言，再写源稿。信息结构决定版式：对比用对齐比较，过程用连接关系，趋势用图表，决策页强调行动和取舍。不默认把每页做成相同的等宽卡片。需要选构图或改善节奏时读 [设计方法](references/design.md)。

## 定位工具，再写源稿

从本 Skill 目录运行：

```bash
python3 scripts/build.py --doctor
```

脚本自动查找 SlideShuttle 仓库、CLI 和随项目提供的 Noto CJK 常规/粗体。复制到其他环境时可传 `--repo /path/to/SlideShuttle`、`--cli /path/to/slide-shuttle` 或 `--font FILE`。缺少运行时时，按诊断中的路径构建或配置；不要假装已经执行。

写入独立的文稿工作目录，使用语义稳定的 ID。先读 [语言与排版约定](references/authoring.md)；以运行时报告的 `languageDoc` 核对需要的高级属性。只有支持的 XML/CSS 子集能编译，浏览器/React 的任意属性不能直接搬过来。

多页稿按项目组织：

```
deck/
  deck.xml          # 入口：尺寸、字体、<Library src>、按顺序的 <Slide src>
  design.xml        # <Library>：全稿共享的 Token / Style / Component
  pages/01-cover.xml
  pages/02-market.xml
  media/
```

先定页序、每页论点和共享设计（入口 + `design.xml`），再写页面。页面文件只放该页内容，元素 ID 只需页内唯一，所以各页可以分别撰写；能并行时把每页交给一个子 Agent，交代该页论点、可用的 Style/Component 和数据来源，让它只改自己的页面文件，并用 `slide-shuttle check deck.xml --page <slide id>` 自检。共享库只由一个 Agent 修改；改动 Token/Style/Component 后要复核所有使用它的页面。页序只看入口，调整顺序改入口，不重命名文件。

[assets/starter/](assets/starter/deck.xml) 是经过渲染验证的五页项目示例，包含不同构图、原生图表和可复用组件。仅在能节省重复工作时复制；可以更换全部主题、页序、组件与布局，不把它当作固定模板。

## 编译、看图、修改

```bash
python3 scripts/build.py /path/to/deck/deck.xml --out /path/to/output/v1
```

输出包含 `presentation.pptx`、`preview/deck.pdf`、逐页 PNG、`index.html`、检查与构建记录。若安装了 Pillow，还会生成 `contact-sheet.png`。输出目录须是新路径；改稿用 `v2` 等新目录，不覆盖已审阅的产物。

先修复编译诊断，再查看每页完整 PNG；用总览检查全稿节奏。按照 [视觉复核](references/review.md) 修正阅读顺序、文字拥挤、意外重叠、对齐与失衡。`build.json` 的 `visualReview: pending` 表示脚本没有替你做视觉判断。编译成功不等于美观，缩略图也不能证明小字可读。

文字溢出时，先改写冗长文案、调整空间分配或拆页，保留用户要求的关键信息。只有设计允许时才缩字号。窄标签可测量；列表符号用内在宽度。源码更新后重新构建并复核受影响的页；共享 Token/Component 改动需要查看所有受影响页面。

## 交付与已有稿件

按用户要求交付 PPTX、PDF、预览和可复用 XML/资源，简要说明设计选择、检查结果与实际限制。原始资料/数值不能因版面优化被悄悄改动。原生 PNG/PDF 已复核，只能据此说明原生渲染；未运行 PowerPoint/WPS 时不声称已在这些应用验收。

处理已有 PPTX 或带导入基线的源稿时读 [已有稿件与导出边界](references/existing-decks.md)。保留 `losses` 和基线，走 `diff`/逐项决策；不要移走旁车、换一个空目录来绕过保护。原生自创稿按普通构建流程推进，不额外引入审批步骤。

私有 `.slide-shuttle/`、比较报告和构建日志可能含历史正文；交付时只选需要的文件。不自动启动预览服务器、发布网页或上传文稿。
