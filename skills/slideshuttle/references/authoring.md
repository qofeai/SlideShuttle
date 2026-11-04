# 语法与排版速查

仓库 `docs/LANGUAGE.md` 是完整语法权威；本页汇总高频写法与防踩坑规则。

## 1. 最小多文件工程模板

`deck.xml`（入口只放尺寸、字体、`<Library src>` 和 `<Slide src>`，不内联页面内容）：
```xml
<Deck title="演示标题" width="1280px" height="720px" font-family="Noto Sans CJK SC" transition="fade">
  <Library src="design.xml"/>
  <Slide src="pages/01-cover.xml"/>
  <Slide src="pages/02-insight.xml"/>
</Deck>
```

`design.xml`（严格遵守字号底线：标题 40–52px、正文 20–26px、注释 16–18px）：
```xml
<Library>
  <Token name="ink" value="#0F172A"/>
  <Token name="muted" value="#475569"/>
  <Token name="accent" value="#0284C7"/>
  <Token name="paper" value="#F8FAFC"/>
  <Style name="frame">width:100%;height:100%;padding:52px 64px;display:flex;flex-direction:column;gap:24px</Style>
  <Style name="kicker">height:28px;font-size:16px;line-height:24px;font-weight:700;color:var(--accent)</Style>
  <Style name="heading">height:64px;font-size:44px;line-height:58px;font-weight:700;color:var(--ink)</Style>
  <Style name="body">font-size:22px;line-height:36px;color:var(--muted)</Style>
  <Style name="chart">font-size:16px;color:#334155;chart-colors:#0284C7 #0D9488 #D97706 #64748B</Style>
</Library>
```

`pages/02-insight.xml`（每页一个焦点、留白充足、详细解释进 `<Notes>`）：
```xml
<Slide id="insight" type="content" background="#F8FAFC">
  <Box id="frame" class="frame">
    <Text id="kicker" class="kicker">01 / KEY MECHANISM</Text>
    <Text id="title" class="heading">让标题直接亮出结论，主体只放核心证据。</Text>
    <Box id="grid" style="display:grid;grid-template-columns:7fr 5fr;gap:48px;height:390px">
      <Shape id="visual" style="height:360px;background:#FFFFFF;border:1px solid #CBD5E1;border-radius:16px"/>
      <Box id="points" style="display:flex;flex-direction:column;justify-content:center;gap:20px">
        <Text id="metric" style="height:108px;font-size:88px;line-height:104px;font-weight:700;color:var(--accent)">437×</Text>
        <Text id="claim" class="body" style="height:120px">用大数字或结构图锁定视线，<br/>用两三行短语点明核心因果。</Text>
      </Box>
    </Box>
  </Box>
  <Notes>
    把完整的背景故事、口语连接词和逐步推导写在这里，保持画面清爽。
  </Notes>
</Slide>
```

---

## 2. 高频语法与防错规则

| 能力 | 正确写法与约束 |
|---|---|
| **布局与剩余空间** | `Box` 默认 `display:block`（border-box）；支持 Flex 与显式 Grid（`grid-template-columns: 7fr 5fr`）。占满剩余宽度必须写 `flex-grow:1;flex-basis:0;min-width:0`（不支持 `flex:1` 简写）。 |
| **字体与字形安全** | Noto Sans CJK SC 含 400（常规）/ 700（粗体），**不支持斜体合成**。数学上下标必须用 `<Span style="vertical-align:sub">1</Span>` / `<Span style="vertical-align:super">2</Span>`，**切勿使用 `₁`、`₂`、`ₙ`、`✗`、`►` 等生僻 Unicode 字符**（会触发 `font has no glyph` 报错；对勾/叉号用 `✓` / `×`，箭头用 `→` 或 `Shape` / `Path`）。 |
| **富文本与列表** | `Text` 内要么只放行内内容（文本、`Span`、`A`、`br`），要么只放 `<P>` / `<List><Item>...</Item></List>`，**行内与块级不能混放**。XML 中 `<`、`>`、`&` 必须转义；避免源码缩进产生多余空格。 |
| **图标 (Icon)** | 用 `python3 scripts/icons.py --list 关键词` 查找，再用 `python3 scripts/icons.py brain zap --out deck/icons` 拷贝；XML 中写 `<Icon id="ic" src="icons/zap.svg" style="width:32px;height:32px;color:#0284C7"/>`。 |
| **矢量图形** | `Shape` 支持 187 种 OOXML 预设（`rect`, `ellipse`, `triangle`, `rightArrow`, `line` 等，可用 `slide-shuttle shapes` 查询）；线条加箭头写 `shape="line" style="border:2px solid #0284C7;marker-end:arrow"`。`Path` 支持 SVG `d` 语法与 `viewBox="100 100"`。 |
| **图表 (Chart)** | `type="bar|line|area|pie|doughnut|scatter"`（横向条形图加 `orient="horizontal"`）；`<Labels>A|B|C</Labels>` 用竖线分隔，`<Series name="...">10 20 30</Series>` 用空格分隔数值。图表样式字号建议 `16px`。 |
| **表格 (Table)** | `<Table columns="2 3 2">` 定义列宽比例，内含 `<Row class="...">` 与 `<Cell colspan="..." rowspan="...">`。单元格字号 **≥ 16px**，只放短语或数字，避免在表格里塞长段落。 |
| **组合与放映** | `<Group>` 在 PPTX 中导出为一层原生组合（不支持嵌套 Group 或组级 opacity）。动画 `<Animate target="id" effect="fade-in|fly-in|wipe-in" trigger="click|with-previous|after-previous"/>` 写在 `<Slide>` 末尾。 |

**不支持的 CSS/Web 特性**：`calc()`、`rem/em`（仅 `letter-spacing` 支持 `em`）、`repeat()/minmax()`、复杂选择器、容器级 `opacity`、JavaScript。

---

## 3. 窄文本测量与已有 PPTX 改稿

- **窄标签测量**：不确定短标签宽度时，先调用 `measure`（注意 `--style` 为 camelCase JSON）：
  ```bash
  slide-shuttle measure --text '标签文字' --style '{"family":"Noto Sans CJK SC","size":22,"lineHeight":32}' --nowrap --font /path/to/NotoSansCJK-Regular.ttc
  ```
- **已有 PPTX 导入与基线保护**：
  - 导入外部 PPTX：`slide-shuttle import original.pptx --out imported --font ...`（生成绝对布局 XML、`losses.json` 与 `.slide-shuttle/base.json`）。
  - 比较差异：`slide-shuttle diff imported/deck.xml --base ... --against external.pptx --font ... --out review.json`。
  - 保留 `.slide-shuttle/` 基线与损失保护，不要通过删除旁车或换空目录绕过检查。
