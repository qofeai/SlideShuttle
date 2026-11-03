# 语言与排版速查

运行 `scripts/build.py --doctor` 找到实际 CLI、字体与 `languageDoc`。仓库 `docs/LANGUAGE.md` 是当前版本的语法权威；本页列出创作最容易踩到的约束，不代替全部 API。

## 可以直接使用

```xml
<Deck width="1280px" height="720px" font-family="Noto Sans CJK SC">
  <Token name="accent" value="#087E6D"/>
  <Style name="title">font-size:48px;line-height:64px;font-weight:700</Style>
  <Slide id="overview" background="#FFFFFF">
    <Box id="frame" style="padding:56px;display:flex;flex-direction:column;gap:28px">
      <Text id="title" class="title" style="height:128px">让标题表达观点，<br/>让主体提供证据。</Text>
      <Box id="columns" style="display:grid;grid-template-columns:7fr 5fr;gap:32px;height:360px">
        <Shape id="evidence" style="height:320px;background:var(--accent);border-radius:16px"/>
        <Text id="explanation" style="height:240px;font-size:24px;line-height:38px">这里写支撑该观点的解释。</Text>
      </Box>
    </Box>
  </Slide>
</Deck>
```

Slide 使用全稿唯一、稳定的 ASCII 语义 ID；普通节点只需在本页内唯一，如 `evidence`。多页稿用入口 `deck.xml` + `<Library src>` + 每页一个 `<Slide src>` 文件，见语言说明的“项目结构”；改一页后用 `check --page <id>` 快速自检。节点支持 Box、Text/Span/br、Image、Shape、Path、Table、Chart、单层 Group，另有 Token、Style、Component/Use。XML 特殊字符应转义；Text 内真实空白会保留，排版时避免源码缩进被当作正文，使用明确的 br 换行。

| 需要 | 使用 |
|---|---|
| 布局 | Box；block、Flex、受限 Grid；border-box；显式列宽/比例、gap、padding、margin |
| 剩余宽度 | flex-grow:1;flex-basis:0;min-width:0，而非 flex:1 简写 |
| 字体 | 实际存在的 family 与 400/700 字重；Noto CJK 包含常规/粗体，不合成斜体；西文与中文用不同字体时写 `font-family:'西文字体', '中文字体'`，两者都要提供 |
| 文字效果 | `text-align:distribute` 分散对齐；`writing-mode:vertical-rl` 竖排标题；Text 的 `color:linear-gradient(...)` 渐变字；`text-shadow` 字形阴影。克制使用，一页一个焦点 |
| 图片 | PNG/JPEG、contain/cover/fill 与 object-position；`border-radius:50%` 圆形头像、`shape="hexagon"` 等预设裁剪、border、阴影 |
| 图标 | `<Icon src="icons/名称.svg" style="width:40px;height:40px;color:#…"/>`；用 `python3 scripts/icons.py 名称… --out 工程/icons` 从 Lucide 复制，`--list 关键词` 查名称 |
| 矢量图形 | Shape 支持全部 OOXML 预设（箭头、星形、对话气泡、流程图、连接线等，`slide-shuttle shapes` 列出名称与 adjust）；Path 接受完整 SVG 路径语法，viewBox="100 100" 是逻辑宽高 |
| 数据 | Chart 有 bar（含横向）/line/area/pie/doughnut/scatter、组合、堆叠、数据标签、数字格式；把字号、颜色和 `chart-colors` 放进一个 Style 让全稿图表一致，Series 通常不写颜色；Labels 用竖线分隔；Series 内空格分隔有限数值；Table 支持合并单元格、富文本单元格、行列样式与单边边框，行高随内容；表头/斑马纹用 Row 的 class 显式写出 |
| 填充 | 纯色/RGBA、任意角度线性渐变或径向渐变（2–8 色标）；页面背景还可用 `url(图片)` |
| 质感 | `box-shadow` 单个外阴影；`border-style:dashed/dotted`；线条/Path 的 `marker-end:arrow` 等箭头；`transform:rotate()` 与镜像只用于叶子 |
| 复合对象 | 一层 Group；不要嵌套 Group 或设置组级透明度 |
| 放映 | Deck `transition="fade"` 统一翻页切换；Slide `type`（cover/contents/section/content/end）；`<Notes>` 写讲稿；`<Animate target effect trigger>` 写在 Slide 末尾，要一起出现的内容放进 Group。每页 1–3 次点击为宜，优先 fade-in、fly-in、wipe-in；用 `render --steps` 检查出场顺序 |

不要使用 React/JS、完整 HTML、CSS 选择器、calc、rem/em、repeat/minmax、滤镜、自动分页等未支持的能力。Span 允许字体、字号、字重、斜体、行高、颜色、字间距、下划线/删除线、上下标和高亮，不承担布局与对齐。Table/Chart/容器没有整体 opacity。新需求先核对语法，再决定用可用构图替代或说明限制。

## 文字测量与列表

CLI 测量样式是 **camelCase JSON**，源文件样式是 CSS 属性名：

```bash
slide-shuttle measure --text '中文 mixed text' \
  --style '{"family":"Noto Sans CJK SC","size":24,"lineHeight":36}' \
  --width 320 --font /path/to/NotoSansCJK-Regular.ttc
```

返回 width、height、lineCount、glyphCount。`--nowrap` 禁止软折行。为文字框留出排版余量；测量成功不保证别的 Office 排版器采用相同折行。

要点列表直接用原生列表，导出后在 PowerPoint 里仍是可编辑的项目符号/编号：

```xml
<Text id="points" style="height:180px;font-size:22px;line-height:34px">
  <List>
    <Item>第一条要点</Item>
    <Item>第二条要点<List type="number"><Item>细分一</Item></List></Item>
  </List>
</Text>
```

段落间距用 `<P style="margin-top:…">` 或 `List` 的 `margin-top`，不要用空行凑。只有当符号本身是装饰图形、需要单独配色排版时，才拆成 Flex 行。不要凭字符数为窄标签猜宽度，先用 measure。

正常正文保留软换行；明确不能拆开的短标签可用 white-space:nowrap，同时确保可用宽度足够。固定高度要容纳实际行数。不要用隐藏错误、截断正文或把一页变成图片来让编译通过。

## 工具与资源

构建脚本自动为 CLI 提供项目自带的 Noto CJK 常规/粗体；指定 `--font` 后使用所提供的字体集，混排多个字体时逐个传入需要的文件。CLI 是严格模式，不自动替代缺失字体。字体和图片资源声明须留在源文件目录内，外部字体可通过 --font 明确传入。

需要直接调用时，CLI 有 measure/check/render/export/import/inspect/diff。check 输出诊断与节点位置；render 写新目录，可带 --pdf；export 写新 PPTX 并生成旁车与回执。工具路径或参数不确定时读取 --help，别发明不存在的子命令。
