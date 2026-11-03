# SlideShuttle 源语言

该格式是独立的 XML。以 HTML/CSS 中的盒子、组合、继承和设计令牌为参考，不解释 HTML，不执行 React 或 JavaScript。每个文件最大 4 MiB、最多 20,000 个 XML 节点，项目合计 64 MiB；1–300 页，每页展开后最多 20,000 个节点，树深度最多 32。DTD 和实体定义不允许。

## 项目结构

一份稿是一个入口文件，加上它引用的共享库和页面文件：

```
deck/
  deck.xml            # 入口：尺寸、字体、共享库、页序
  design/system.xml   # <Library>：Token / Style / Component / Font / Asset
  pages/cover.xml     # 一页一个文件，根元素是 <Slide>
  media/  fonts/
```

```xml
<Deck title="年度总结" width="1280px" height="720px" font-family="Noto Sans CJK SC">
  <Font family="Noto Sans CJK SC" src="fonts/NotoSansCJK-Regular.ttc"/>
  <Library src="design/system.xml"/>
  <Slide src="pages/cover.xml"/>
  <Slide src="pages/market.xml"/>
</Deck>
```

- 页序只由入口中 `<Slide>` 的顺序决定，文件名不影响顺序；页面身份是页面文件里的 `id`。
- `<Library src>` 和 `<Slide src>` 只有 `src`，不能带内容。入口里也可以直接内联 `<Slide id="…">…</Slide>`，小稿可以只有一个文件。
- 所有路径（页面、共享库、字体、图片）都相对入口所在目录，必须留在该目录内：不能是绝对路径，不能含 `..`，符号链接也不能指向目录外。
- 共享定义只在入口和 `<Library>` 中；`<Library>` 不能再引用其他文件或包含 `<Slide>`。同名定义报错，不互相覆盖。
- 错误与诊断带文件名，例如 `pages/market.xml line 3, column 5: …`；`check` / `render` 可用 `--page <slide id>` 只排版、只渲染一页，仍加载入口、共享库和字体。

## 文档与资源

根节点：`Deck title="…" width="1280px" height="720px" font-family="…"`。长度支持 px、pt、emu，0 可省单位；1 px = 9525 EMU，1 pt = 4/3 px。页面尺寸最大 16384 px。颜色支持 `#RGB`、`#RRGGBB`、`#RRGGBBAA`、black、white、transparent。

`Slide` 需要全稿唯一的 `id`；`background` 可为纯色、渐变或 `url(<图片>)`（铺满页面并裁切，PPTX 中为原生页面背景）。普通节点必须有稳定的 `id`，只需在本页内唯一，允许 ASCII 字母、数字和 `_ - / .`；诊断以页面 ID 加节点 ID 定位。渲染诊断带文件、行列和节点 ID；解析与布局错误同样以 `文件 line L, column C: …` 指向出错元素，一次报告各兄弟元素的独立错误（最多 50 条）。

| 元素 | 属性 / 内容 |
|---|---|
| `Font` | `family`、`src`，显式提供实际字体文件；`family` 必须是文件内的实际 family 名，不作别名，否则加载即报错 |
| `Asset` | `id`、`src`，图片路径别名 |
| `Token` | `name="accent" value="#09A88F"`，样式通过 `var(--accent)` 引用 |
| `Style` | `name`，内容是属性声明；节点 `class="a b"` 合并，inline style 最后覆盖；`padding`、`margin`、`gap`、`border`、`background-color` 按书写顺序与长写属性层叠，后写者胜，同 CSS。Token 可嵌套引用，深度上限 16、展开结果上限 64 KiB |
| `Component` | `name`，恰好一个根节点 |
| `Use` | `id`、`component`、静态字符串参数；模板中 `${name}` 替换 |

组件内部 ID 加实例前缀，例如 `first/card`。参数替换作用于模板的 inline style、文本和图片 src；不实现计算、条件、循环、事件、动态状态和完整 CSS 选择器。

## 节点

| 节点 | 能力 |
|---|---|
| `Box` | 布局与背景，任意支持的子节点 |
| `Group` | 与 Box 同样布局；PPTX 输出一层原生组；不支持嵌套 Group、组旋转或组级透明度 |
| `Text` | 行内内容（文本、`Span`、`A`、`br`），或只含 `P` / `List` 的段落内容；保留作者换行并自动折行，见下文“富文本” |
| `Image` | `src` 为 Asset ID 或相对路径；PNG/JPEG；`object-fit` contain / cover / fill 与 `object-position`；`border-radius`（`50%` 为圆形）或 `shape="hexagon"` 等任意预设裁剪（`adjust` 同 Shape）；可加 border、box-shadow、transform；没有背景 |
| `Icon` | `src` 为单色 SVG 图标（如 Lucide），按 `color` 着色，等比放入盒子（默认 24×24）；支持 path / circle / ellipse / rect / line / polyline / polygon / g，不支持 transform、文字、渐变；PPTX 中为可编辑的自定义形状 |
| `Shape` | `shape` 为 187 种 OOXML 预设之一（默认 rect），如 triangle、rightArrow、star5、wedgeRoundRectCallout、bentConnector3；`adjust` 为预设调整值。完整列表与默认值见 [预设形状](SHAPES.md) 或 `slide-shuttle shapes` |
| `Path` | `d` 为完整 SVG 路径语法（相对命令、H/V、S/T、Q、A 弧线均可）；`viewBox="100 100"` 为逻辑宽高 |
| `Table` | `Row` 内 `Cell`；`columns="2 1"` 为列宽比例；单元格可含富文本与列表、`colspan` / `rowspan`；样式按 表格 → 行 → 单元格 继承，见下文“表格” |
| `Chart` | `type="bar|line|area|pie|doughnut|scatter"`；`Labels` 用 `|` 分隔；`Series name="…"` 内空格分隔有限数值；最多 8 系列、100 分类；选项见“图表” |

表格和图表同时支持本地预览与原生 PPTX。图表导出缓存数值以及嵌入的 `.xlsx` 工作簿，并固定绘图区、图例、标题位置和坐标轴刻度，Office 中与预览排版一致。

## 布局属性

全部盒子采用 border-box。默认 display 是 block。当前属性值中不支持 calc、viewport 单位、rem/em、完整 minmax/repeat、媒体查询、自动分页和复杂滤镜。放映动画见“放映”。

| 属性组 | 支持 |
|---|---|
| 尺寸 | width/height: px/pt/emu、%、auto；min/max-width/height |
| 盒模型 | padding / margin 的 1–4 值与四边属性；margin 允许 auto |
| 定位 | position: relative / absolute；left/top/right/bottom |
| Flex | display:flex；row / column；flex-grow、flex-shrink、flex-basis；gap；align-items/self；justify-content |
| Grid | 显式 px/pt/emu、正数 fr、auto 轨道；grid-template-columns/rows；gap；grid-column/row 的 line、span、`start / end` |
| 填充 | background/background-color：纯色、`linear-gradient(<角度>deg 或 to top/right/bottom/left, 色标…)`、`radial-gradient([circle,] 色标…)`；2–8 个色标，色标可带 `0%–100%` 位置和透明度，缺省位置均分；径向渐变从中心到最远角 |
| 边线 | `border:1px solid|dashed|dotted #D0D8DE`，或 border-width、border-style、border-color；非负 border-radius。线条与 Path 的 border 是描边，不占布局空间 |
| 线帽与拐角 | `stroke-linecap: butt / round / square`、`stroke-linejoin: miter / round / bevel`，作用于描边 |
| 线端 | `marker-start` / `marker-end`：arrow、stealth、diamond、oval、none；只用于 `shape="line"` 与 Path，大小随线宽 |
| 阴影 | `box-shadow:<x> <y> [<模糊>] <颜色>`（单个外阴影，模糊 0–200px）；作用于形状、图片和有背景或边框的 Text 盒子；PDF 中阴影以图像嵌入 |
| 变换 | `transform:rotate(<角度>deg)`、`scaleX(-1)`、`scaleY(-1)`，按 CSS 从右到左组合，绕自身中心，不影响布局；只用于叶子节点，不用于容器、Table、Chart，以及带背景或边框的 Text；越界检查按旋转后的外框 |
| 透明度 | opacity:0–1，仅叶子绘图对象；Table/Chart/容器不接受 opacity |
| 图片 | object-fit:contain / cover / fill；object-position 关键字或百分比（默认居中）|
| 文字 | font-family、font-size、font-weight:400/700、font-style:normal/italic、line-height、color、text-align:left/center/right/justify |
| 换行 | white-space:normal / nowrap / pre-wrap；当前 normal 与 pre-wrap 都保留 XML 文本中的空白，不实现浏览器空白折叠 |

字体必须真的包含所请求的字重/斜体 face；不合成。`Span` 只接受 font-family/size/weight/style、line-height、color、letter-spacing、text-decoration、vertical-align（super/sub）与 background-color（高亮）。颜色、字重、字体继承；行高随字号按继承比率变化，显式 line-height 可用长度或倍数。布局使用 f32 并关闭 Taffy 像素取整；PPTX 到最后一步才转换整数 EMU。

完全空的 Text/Span 内容不产生行框，文字测量为 0×0、0 行；显式尺寸、背景和边框仍按盒子布局保留。明确输入的空格或换行属于内容，仍可占据行距。PPTX 导入中的纯空文本容器不会把线条/形状转成文字，空占位符也不会凭空增加一行。

`check` 报告文本溢出、页面越界、图表类别标签宽于所在分类或图表文字超出图表，以及缩窄 3% 后换行高度超过容器的紧排版警告。错误阻止渲染和导出。目前不对所有物体重叠作审美或遮挡判定，尚未实现自动区域冻结和碰撞修复。装饰性重叠由作者自行设计。

0.2.0 导出 PPTX 时，未指定宽度或宽度 auto、采用内在宽度且没有软折行的紧标签禁用软折行，避免接收端在末尾拆出单个字母；显式宽度正文维持换行。`white-space:nowrap` 也输出禁用软折行。已有显式换行不会因此删除。这项策略不能保证所有 Office 渲染器的字形和文字边界相同。

## 表格

```xml
<Style name="head">background:#0F172A;color:#FFFFFF;font-weight:700</Style>
…
<Table id="plan" columns="2 1 1" style="width:560px;font-size:16px;line-height:24px;border:1px solid #CBD5E1">
  <Row class="head"><Cell rowspan="2">项目</Cell><Cell colspan="2" style="text-align:center">季度</Cell></Row>
  <Row class="head"><Cell>Q1</Cell><Cell>Q2</Cell></Row>
  <Row style="height:56px"><Cell><List><Item>新客户</Item><Item>续约率</Item></List></Cell><Cell>12%</Cell><Cell style="background:#DCFCE7">18%</Cell></Row>
  <Row style="border-top:2px dashed #DC2626"><Cell>合计</Cell><Cell>30%</Cell><Cell>36%</Cell></Row>
</Table>
```

- 行高随内容自动撑开；Row 的 `height` 是最小高度。Table 设了高度时，富余空间分给未指定高度的行；内容放不下报 `table_overflow`。
- 合并单元格必须恰好铺满网格：每行填满所有未被上方 rowspan 占据的列，否则报错。
- Row 与 Cell 可设文字属性、`background`（纯色）、`padding`、`vertical-align`（top / middle / bottom，默认 middle）、`border` 与 `border-top/right/bottom/left`（`<宽度> solid|dashed|dotted <颜色>`）；Row 另可设 `height`。可用 `class` 复用样式，例如表头与斑马纹。
- Table 的 `border` 是默认网格线（不写时为 1px #D2D9E3，`border:0 solid #000` 去掉），`background` 铺在单元格下面。
- 相邻单元格的公共边只画一条：明确设置的边框优先于默认网格，其次取更宽者，再其次取上方或左侧单元格的；PPTX 写入的也是同一结果。
- PPTX 为原生表格（`gridSpan` / `rowSpan` / `hMerge` / `vMerge`、逐格边距、对齐、填充与四边边框）；导入时同样还原，表格主题样式除外。

## 图表

```xml
<Style name="chart">font-size:16px;color:#536B78;chart-colors:var(--accent) #9B6B32 #6C8290</Style>
…
<Chart id="revenue" class="chart" title="季度营收" data-labels="value" format="#,##0&quot;万&quot;" style="height:360px">
  <Labels>Q1|Q2|Q3|Q4</Labels>
  <Series name="收入">1200 1800 1500 2600</Series>
  <Series name="成本" type="line">900 1100 1300 1500</Series>
</Chart>
```

- `type`：`bar`（默认，`orient="horizontal"` 为条形图，首个分类在上）、`line`、`area`、`pie`、`doughnut`、`scatter`。散点图的 `Labels` 是数值 x。
- 组合图：bar / line / area 图表里，Series 用 `type` 换成另外两种；横向条形图不能组合。
- `stack="normal|percent"`：bar / area 图表中与图表同类型的系列堆叠；`percent` 坐标轴固定 0–100% 且不能组合，值不能为负。
- `legend="auto|top|right|bottom|left|none"`：`auto` 在多系列或饼图/环形图时放底部，单系列不显示。
- `data-labels="value"`；饼图/环形图另有 `percent`、`category`。面积系列不支持数据标签。柱内与扇区内的标签自动选黑或白。
- 数值轴 `min` / `max` / `step` 可选，缺省取整到 1、2、2.5、5 × 10ⁿ 并保证刻度不挤；bar / line / area 默认包含 0，需要放大差异时设 `min`。散点图两轴都按数据范围。
- `format` 是 Excel 数字格式的子集：`0`、`0.0`、`#,##0.00`、`0%`、`0.##`，以及引号内或 `$`、`¥` 等前后缀，用于坐标轴和数据标签。
- `grid="false"` 去掉网格线；`title="…"` 加粗标题；`smooth="true"`、`markers="true"` 只用于含折线系列的图表。
- 颜色：Series `color`（饼图/环形图用 `colors`，每个扇区一个）；缺省按 `chart-colors`（可放进 Style，跨页一致）循环取色，再缺省为内置 8 色。文字颜色、字号和字体继承自普通文字样式，网格线与坐标轴线由文字颜色淡化得出；`background` 与实线 `border` 作为图表区底色与边框。
- 导入时还原上述全部类型和选项；三维、股价、雷达等其它图表类型、次坐标轴、散点连线和趋势线会作为损失报告。

## 放映

```xml
<Deck … transition="fade">
  <Slide id="market" type="content" transition="push left fast">
    <Text id="claim" …>…</Text>
    <Group id="proof" …>…</Group>
    <Animate target="claim" effect="fade-in"/>
    <Animate target="proof" effect="fly-in" direction="up" trigger="after-previous" duration="0.6s" easing="ease-out"/>
    <Notes>
      先说结论。
      再指出三个证据。
    </Notes>
  </Slide>
```

这些只影响放映和 PPTX，不改变静态页面、PNG 和 PDF。

- 页面类型 `type="cover|contents|section|content|end"`，在 PPTX 中对应同名版式（封面为 title、章节为 secHead 等），便于在 PowerPoint 中按版式识别页面。
- 切换 `transition="<种类> [方向] [速度]"`：种类为 fade、push、wipe、cover、split、zoom、cut；push / wipe / cover 可带 up/down/left/right（缺省 left）；速度 fast / medium / slow。Deck 上的 `transition` 是各页默认值，Slide 写 `none` 取消。
- `<Animate>` 按书写顺序编排，写在 Slide 的直接子级。`target` 是本页的叶子元素或 Group；带子元素的 Box 不能整体动画，需要一起动的内容放进 Group。
  - 入场：appear、fade-in、fly-in、zoom-in、wipe-in、float-in、peek-in、rise-in。
  - 强调：pulse、grow-shrink（放大 1.5 倍并保持）、spin、teeter、fill-color（`color` 必填，保持）、transparency（`amount` 0–1 必填，保持）、color-pulse（`color` 必填）。
  - 退出：disappear、fade-out、fly-out、zoom-out、wipe-out、float-out。
  - 路径：motion-path，`path` 是相对元素当前位置的 SVG 路径（px），从 `M 0 0` 开始，只有一个子路径。
- `trigger`：`click`（默认，开始新的一次点击）、`with-previous`（与上一项同时）、`after-previous`（上一项结束后）。页面第一项不是 click 时，进入页面就自动播放。
- `direction` 只用于 fly、wipe、peek、float（float 只有 up/down），表示运动方向，例如 `fly-in direction="up"` 从页面下方向上飞入；缺省 up。`duration`、`delay` 写 `500ms` 或 `0.5s`；appear / disappear 没有时长。`easing` 为 linear / ease-in / ease-out / ease-in-out；`repeat` 为 1–100。
- 首个动画是入场效果的元素在放映开始时隐藏。`render --steps` 另存每页放映的每一步：`slide-002-step-0.png` 是第一次点击前，`step-1` 是第一次点击后，依此类推。中间过程不画，只画每步结束时的状态（显隐、透明度、填充色、放大和路径终点）。
- `<Notes>` 是演讲者备注，纯文本，每个非空行是一段，在 PPTX 中写成备注页。
- 导入还原上述全部内容。原生组导入后展开，组上的动画改为在每个成员上播放，并报告为损失；按段落播放、点击形状触发等其它动画同样作为损失报告。

## 富文本

一个 `Text` 要么只有行内内容（一个段落），要么只有 `P` 与 `List` 块；两者不能混放。

```xml
<Text id="body" style="width:600px;height:320px;font-size:22px;line-height:34px;vertical-align:middle">
  <P>H<Span style="vertical-align:sub">2</Span>O、<Span style="text-decoration:underline">下划线</Span>、
     <Span style="background-color:#FDE68A">高亮</Span>、<A href="https://example.com">链接</A></P>
  <P style="text-align:right;margin-top:12px">右对齐的段落</P>
  <List type="number" start="3" style="margin-top:12px">
    <Item>第三项
      <List><Item>嵌套的项目符号</Item></List>
    </Item>
    <Item>第四项</Item>
  </List>
</Text>
```

| 内容 | 说明 |
|---|---|
| `P` | 段落；`style` 只接受 `text-align`、`margin-top`（段前距，第一段也生效） |
| `List` | `type="bullet"`（默认）或 `number`，`start` 为起始编号；`style` 的 `text-align` 作用于各项，`margin-top` 只加在列表之前；最多嵌套 5 层 |
| `Item` | 一个列表项：行内内容，之后可跟嵌套 `List`。标记与悬挂缩进按该项文字字号计算，每层 1.5em；项目符号依层级为 • 或 –，编号为 `1.` |
| `A` | `href` 只允许 http、https、mailto；使用主题链接色 `#2563EB` 和下划线，不能另设 color / text-decoration；PDF 中可点击，PPTX 为原生超链接 |
| `Span` 行内样式 | `text-decoration: underline / line-through / none`（可组合）；`vertical-align: super / sub / baseline`（字号缩为 0.65）；`letter-spacing`（px、pt 或 em，±2em 内）；`background-color` 为文字高亮 |
| `Text` 的 `vertical-align` | `top`（默认）/ `middle` / `bottom`，内容在文字框内的垂直位置；只能用于 Text |

源码里内容两端的换行与缩进是排版用的空白，不算正文；内容中间的空白保留。PPTX 中这些都写成原生结构：段落属性、`buChar` / `buAutoNum` 项目符号与编号、`u` / `strike` / `baseline` / `spc` / `highlight`、`hlinkClick` 外部链接、`anchor` 垂直对齐；导入时同样重建，不再记为损失。

窄文本可先通过 CLI `measure` 或 WASM `measureText` 获取实际字体的宽高与行数。推荐列表符号使用自动宽度，正文用 Flex 分配剩余空间，参考 [组件示例](../examples/components.xml)。这些 API 不改变语言 v1，详见 [渲染说明](RENDERING.md)。

## 导入边界

PPTX 限压缩 64 MiB、4096 个条目、总展开 256 MiB、单 XML 16 MiB。拒绝 ZIP 路径穿越、重复条目、符号链接、DTD 与外部实体；外部关系只记录，不获取。图片、字体与渲染表面有独立大小限制。这些是资源预算，不是完整的恶意 Office 文件安全审计。

导入重建文本、常用形状、受限自定义路径、图片、基本表格，以及有数值缓存的柱状/折线图；使用布局/母版定位占位符，现已解析主题色、颜色映射、常用变换、透明度与默认文字级联；完整主题效果、复杂段落和母版装饰仍有限。原生组几何折算后展开，不恢复组件。页面切换、动画、备注和页面类型按“放映”一节还原；字体/段落/单元格的复杂局部格式、继承装饰、批注、媒体和其他扩展不能保证恢复。

`losses.json` 是已经识别的损失；原生对象完整结构摘要与整包部件清单还用于保守比较。没有列出损失不等于任意文件通过视觉等价认证。导入后的新导出必须检查旁车比较结果，不支持把“再次编译成功”解释为无损往返。

主题导入的准确支持范围、验证与限制见 [0.3.0 导入说明](OOXML-IMPORT.md)。
