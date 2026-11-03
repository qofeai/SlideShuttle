# 语言与排版速查

运行 `scripts/build.py --doctor` 找到实际 CLI、字体与 `languageDoc`。仓库 `docs/LANGUAGE.md` 是当前版本的语法权威；本页列出创作最容易踩到的约束，不代替全部 API。

## 可以直接使用

```xml
<Deck version="1" width="1280px" height="720px" font-family="Noto Sans CJK SC">
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

所有 Slide/普通节点使用全局唯一、稳定的 ASCII 语义 ID，如 `market/evidence`。节点支持 Box、Text/Span/br、Image、Shape、Path、Table、Chart、单层 Group，另有 Token、Style、Component/Use。XML 特殊字符应转义；Text 内真实空白会保留，排版时避免源码缩进被当作正文，使用明确的 br 换行。

| 需要 | 使用 |
|---|---|
| 布局 | Box；block、Flex、受限 Grid；border-box；显式列宽/比例、gap、padding、margin |
| 剩余宽度 | flex-grow:1;flex-basis:0;min-width:0，而非 flex:1 简写 |
| 字体 | 实际存在的 family 与 400/700 字重；Noto CJK 包含常规/粗体，不合成斜体 |
| 图片 | PNG/JPEG、contain/cover/fill；src 为工程内路径或 Asset ID，圆角/边框放外层盒子，但图片本身不裁圆角 |
| 矢量图形 | rect/ellipse/line；Path 仅绝对 M/L/C/Z，viewBox="100 100" 是逻辑宽高 |
| 数据 | Chart 仅 bar/line；Labels 用竖线分隔；Series 内空格分隔有限数值；Table 为矩形纯文字单元格 |
| 填充 | 纯色/RGBA 或两个不透明色标的左右/上下线性渐变 |
| 复合对象 | 一层 Group；不要嵌套 Group 或设置组级透明度 |

不要使用 React/JS、完整 HTML、CSS 选择器、calc、rem/em、repeat/minmax、box-shadow、滤镜、自动分页等未支持的能力。Span 仅允许字体、字号、字重、斜体、行高、颜色，不承担布局与对齐。Table/Chart/容器没有整体 opacity。新需求先核对语法，再决定用可用构图替代或说明限制。

## 文字测量与列表

CLI 测量样式是 **camelCase JSON**，源文件样式是 CSS 属性名：

```bash
slide-shuttle measure --text '中文 mixed text' \
  --style '{"family":"Noto Sans CJK SC","size":24,"lineHeight":36}' \
  --width 320 --font /path/to/NotoSansCJK-Regular.ttc
```

返回 width、height、lineCount、glyphCount。`--nowrap` 禁止软折行。为文字框留出排版余量；测量成功不保证别的 Office 排版器采用相同折行。

不要凭字符数为窄标签和列表符号猜宽度。可复用以下组件，让符号按字体自动占位、正文获取余下空间：

```xml
<Component name="BulletRow">
  <Box id="row" style="display:flex;align-items:start;gap:12px">
    <Text id="marker" style="flex-shrink:0;white-space:nowrap">•</Text>
    <Text id="content" style="flex-grow:1;flex-basis:0;min-width:0">${text}</Text>
  </Box>
</Component>
```

正常正文保留软换行；明确不能拆开的短标签可用 white-space:nowrap，同时确保可用宽度足够。固定高度要容纳实际行数。不要用隐藏错误、截断正文或把一页变成图片来让编译通过。

## 工具与资源

构建脚本自动为 CLI 提供项目自带的 Noto CJK 常规/粗体；指定 `--font` 后使用所提供的字体集，混排多个字体时逐个传入需要的文件。CLI 是严格模式，不自动替代缺失字体。字体和图片资源声明须留在源文件目录内，外部字体可通过 --font 明确传入。

需要直接调用时，CLI 有 measure/check/render/export/import/inspect/diff。check 输出诊断与节点位置；render 写新目录，可带 --pdf；export 写新 PPTX 并生成旁车与回执。工具路径或参数不确定时读取 --help，别发明不存在的子命令。
