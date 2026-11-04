---
name: slideshuttle
description: "用 SlideShuttle 的静态 XML、Grid/Flex 和原生渲染，把材料做成论点清楚、构图讲究、可编辑的演示稿（.qppt / .pptx / .pdf / .png）。适用于从材料生成演示、重写故事线、优化页面层级与构图，或明确要求 SlideShuttle 的任务。"
---

# 用 SlideShuttle 设计演示稿

演示稿是**讲者身后的视觉论证**：观众隔着距离、跟着讲者的节奏一页页看，每页只有几秒。按顺序做好三层，后一层弥补不了前一层：

1. **论证**：全稿回答一个问题，每页推进一个判断，只读标题就能复述整个故事。
2. **画布**：每页一个视觉主角，构图用满整张画布，留白是安排出来的。
3. **系统**：全稿共享网格、字号阶梯、颜色职责和标题位置，翻页时只有内容在变。

## 红线

1280×720 画布。标 ✓ 的项由 `scripts/build.py` 的 `designAudit` 自动审计，其余靠看图判断。其他文件不再重复这些数字，以这里为准。

| 项 | 标准 |
|---|---|
| 字号 ✓ | 主角 56–240px；标题 40–52px；正文 20–26px，**不得小于 20px**；图注、来源、表格 16–18px，**不得小于 16px** |
| 字数 ✓ | 单页可见文字 60–150 字，**上限 220 字**；单个文本框 1–3 行短语，**不超过 7 行** |
| 留白 | 留白率 ≥ 40%，并且是成块、有意安排的；外边距 48–64px，主分栏间距 36–56px |
| 结构 ✓ | 单页渲染节点 **≤ 26**；并列模块 ≤ 3；不加用户没要求的页码、页脚、水印 |
| 焦点 | 每页 1 个主角，占 50–65% 视觉权重 |
| 排版 ✓ | 多行文字的行长 **≤ 32em**；多行文字的行高，40px 以下 ≥ 1.25 倍、40–72px ≥ 1.05 倍；全稿 48px 以下的字号 **≤ 5 级**；内容页标题在同一个锚点；每页都有 `id="title"` |
| 颜色 ✓ | 黑白灰之外只用 1 个强调色，只给证据、本方数据和需要行动的内容；全稿饱和色相超过 2 个时报警（第二个只留给好坏对比）；文字对比度 **≥ 4.5:1**，大字（24px 以上，或 18.7px 以上的粗体）**≥ 3:1** |

放不下时依次这样处理：先删词、改成短语、把解释移进 `<Notes>`；再调整空间分配或拆页；**绝不靠缩小字号**。

## 流程

**1. 简报**：先读 [design.md](references/design.md)，然后在工程目录写 `brief.md`，写清场合、受众及其语言和地区、受众要做的决定、一句话结论、故事线和每页的标题论断（模板见 [assets/starter/brief.md](assets/starter/brief.md)）。只读标题能复述整个论证后，再动 XML。缺少会影响结论的事实时，集中问用户；非关键偏好做合理假设，写进简报。

**2. 源稿**：按多文件工程写，语法遇到问题查 [syntax.md](references/syntax.md)。

```
deck/
  brief.md        # 简报与页面计划
  deck.xml        # 尺寸、字体、<Library src="design.xml"/>、按序排列的 <Slide src>
  design.xml      # Token / Style / Component：网格、字号阶梯、颜色职责、重复结构
  pages/01-cover.xml …
```

先定 `deck.xml` 和 `design.xml`，再写页面。每页标题统一用 `<Text id="title">`。可以复制 [assets/starter/](assets/starter/deck.xml) 起步，但要换掉主题、页序和构图。页数多时，可以把每页交给一个子 Agent，交代清楚该页的论断、证据和可用组件，让它只改自己的页面文件。

**3. 构建**：

```bash
python3 scripts/build.py --doctor
python3 scripts/build.py deck/deck.xml --out out/v1
```

产物包括 `presentation.qppt`、`.pptx`、`preview/deck.pdf`、逐页 PNG、`contact-sheet.png`、`outline.md`，以及 `build.json`（含 `diagnostics` 和 `designAudit`）。两者都要清零。只改一页时，可以用 `slide-shuttle check deck.xml --page <id> --font …` 快速自检。

**4. 复核与交付**：按 [review.md](references/review.md) 读大纲、看总览，再逐页看 PNG；改稿用新目录（`v2`）重新构建。优先交付 `.qppt`，按需附上 `.pptx`、PDF 和预览图，说明故事线、设计取舍和实际验证范围。

## 边界

- 不悄悄改动原始事实和数字。演示数据、推测要标明，写在讲稿和简报里，不在页面上加免责声明。
- 没在 PowerPoint 或 WPS 里打开过，就不声称已通过 Office 验收。
- 已有 PPTX 要保留 `losses` 和基线，走 `diff` 逐项决策（见 syntax.md 第 3 节）；`.slide-shuttle/` 可能含历史正文，不作为交付附件。
