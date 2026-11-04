# 语法速查

写源稿遇到语法问题时查这里。完整语法以仓库的 `docs/LANGUAGE.md` 为准；设计原则见 [design.md](design.md)，数字红线见 SKILL.md。

## 1. 最小多文件工程模板

`deck.xml`（入口只放尺寸、字体、`<Library src>` 和 `<Slide src>`，不内联页面内容）：
```xml
<Deck title="演示标题" width="1280px" height="720px" font-family="Noto Sans CJK SC" transition="fade">
  <Library src="design.xml"/>
  <Slide src="pages/01-cover.xml"/>
  <Slide src="pages/02-insight.xml"/>
</Deck>
```

`design.xml`：
```xml
<Library>
  <Token name="paper" value="#F7F5F0"/>
  <Token name="ink" value="#1B1F24"/>
  <Token name="muted" value="#5E6670"/>
  <Token name="rule" value="#D9D4CA"/>
  <Token name="accent" value="#D9542B"/>
  <Token name="contrast" value="#B5AEA4"/>
  <Style name="page">width:1280px;height:720px;padding:56px 64px 72px 64px;display:flex;flex-direction:column;gap:40px</Style>
  <Style name="body">flex-grow:1;flex-basis:0;min-height:0</Style>
  <Style name="title">font-size:40px;line-height:54px;font-weight:700;color:var(--ink)</Style>
  <Style name="text">font-size:22px;line-height:34px;color:var(--muted)</Style>
  <Style name="caption">font-size:16px;line-height:24px;color:var(--muted)</Style>
  <Style name="figure">font-size:120px;line-height:128px;font-weight:700;color:var(--ink)</Style>
  <Style name="chart">font-size:16px;color:var(--muted);chart-colors:var(--accent) var(--contrast)</Style>
</Library>
```

`pages/03-evidence.xml`：
```xml
<Slide id="evidence" type="content" background="#F7F5F0">
  <Box id="frame" class="page">
    <Text id="title" class="title">第 4 周起，会员与非会员的复购差距持续拉开</Text>
    <Box id="grid" class="body" style="display:grid;grid-template-columns:8fr 4fr;gap:56px">
      <Chart id="chart" type="line" class="chart" legend="top" min="0" max="50" format="0&quot;%&quot;" style="height:100%">
        <Labels>第 1 周|第 2 周|第 3 周|第 4 周</Labels>
        <Series name="会员">16 18 20 26</Series>
        <Series name="非会员">15 17 18 19</Series>
      </Chart>
      <Box id="reading" style="display:flex;flex-direction:column;justify-content:flex-end;gap:20px">
        <Text id="gap" class="figure" style="color:var(--accent)">+7<Span style="font-size:32px"> 个百分点</Span></Text>
        <Text id="reading-note" class="text">差距在第 4 周后才出现</Text>
        <Text id="source" class="caption">来源：试点门店 POS，2026.07–09</Text>
      </Box>
    </Box>
  </Box>
  <Notes>
    这页要说的一句话、证据怎么读、数据口径，以及过渡到下一页的一句。
  </Notes>
</Slide>
```

---

## 2. 高频语法与防错规则

| 能力 | 正确写法与约束 |
|---|---|
| **布局与剩余空间** | `Box` 默认 `display:block`（border-box）；支持 Flex 与显式 Grid（`grid-template-columns: 7fr 5fr`）。占满剩余宽度写 `flex-grow:1;flex-basis:0;min-width:0`，纵向占满剩余高度写 `flex-grow:1;flex-basis:0;min-height:0`（不支持 `flex:1` 简写）。`justify-content` 支持 flex-start / center / flex-end / space-between，`align-items` 支持 end；不支持 `align-content`。 |
| **文字尺寸** | Text 省略 height 时按实际行数测量，这是默认写法。只有需要固定占位时才写 height，而且必须容纳全部行数。 |
| **字体与字形安全** | Noto Sans CJK SC 含 400（常规）/ 700（粗体），**不支持斜体合成**。数学上下标必须用 `<Span style="vertical-align:sub">1</Span>` / `<Span style="vertical-align:super">2</Span>`，**切勿使用 `₁`、`₂`、`ₙ`、`✗`、`►` 等生僻 Unicode 字符**（会触发 `font has no glyph` 报错；对勾/叉号用 `✓` / `×`，箭头用 `→` 或 `Shape` / `Path`）。 |
| **富文本与列表** | `Text` 内要么只放行内内容（文本、`Span`、`A`、`br`），要么只放 `<P>` / `<List><Item>...</Item></List>`，**行内与块级不能混放**。XML 中 `<`、`>`、`&` 必须转义；避免源码缩进产生多余空格。 |
| **图标 (Icon)** | 用 `python3 scripts/icons.py --list 关键词` 查找，再用 `python3 scripts/icons.py brain zap --out deck/icons` 拷贝；XML 中写 `<Icon id="ic" src="icons/zap.svg" style="width:32px;height:32px;color:#0284C7"/>`。 |
| **矢量图形** | `Shape` 支持 187 种 OOXML 预设（`rect`, `ellipse`, `triangle`, `rightArrow`, `line` 等，可用 `slide-shuttle shapes` 查询）；线条加箭头写 `shape="line" style="border:2px solid #0284C7;marker-end:arrow"`。`Path` 支持 SVG `d` 语法与 `viewBox="100 100"`。 |
| **图表 (Chart)** | `type="bar|line|area|pie|doughnut|scatter"`（横向条形图加 `orient="horizontal"`）；`<Labels>A|B|C</Labels>` 用竖线分隔，`<Series name="...">10 20 30</Series>` 用空格分隔数值。 |
| **表格 (Table)** | `<Table columns="2 3 2">` 定义列宽比例，内含 `<Row class="...">` 与 `<Cell colspan="..." rowspan="...">`。单元格只放短语或数字。 |
| **组件参数** | `Use` 的参数是纯文本，原样代入 `${name}`；参数里写 `&lt;br/&gt;` 会直接显示成字面量。需要换行时让文字按宽度自然折行，或者把内容拆成两个参数。 |
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
