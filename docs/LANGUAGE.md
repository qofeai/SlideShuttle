# SlideShuttle Language v1

该格式是独立的 XML 文档。以 HTML/CSS 中的盒子、组合、继承和设计令牌为参考，不解释 HTML，不执行 React 或 JavaScript。源文件最大 4 MiB，1–300 页，展开后最多 20,000 个节点，树深度最多 32。DTD 和实体定义不允许。

## 文档与资源

根节点：`Deck version="1" title="…" width="1280px" height="720px" font-family="…"`。长度支持 px、pt、emu，0 可省单位；1 px = 9525 EMU，1 pt = 4/3 px。页面尺寸最大 16384 px。颜色支持 `#RGB`、`#RRGGBB`、`#RRGGBBAA`、black、white、transparent。

`Slide` 需要全局唯一 `id`，背景为纯色。所有普通节点必须有稳定、全局唯一的 `id`，允许 ASCII 字母、数字和 `_ - / .`。渲染诊断带源文件行列和节点 ID。

| 元素 | 属性 / 内容 |
|---|---|
| `Font` | `family`、`src`，显式提供实际字体文件 |
| `Asset` | `id`、`src`，图片路径别名 |
| `Token` | `name="accent" value="#09A88F"`，样式通过 `var(--accent)` 引用 |
| `Style` | `name`，内容是属性声明；节点 `class="a b"` 合并，inline style 最后覆盖 |
| `Component` | `name`，恰好一个根节点 |
| `Use` | `id`、`component`、静态字符串参数；模板中 `${name}` 替换 |

组件内部 ID 加实例前缀，例如 `first/card`。参数替换作用于模板的 inline style、文本和图片 src；不实现计算、条件、循环、事件、动态状态和完整 CSS 选择器。

## 节点

| 节点 | 能力 |
|---|---|
| `Box` | 布局与背景，任意支持的子节点 |
| `Group` | 与 Box 同样布局；PPTX 输出一层原生组；不支持嵌套 Group、组旋转或组级透明度 |
| `Text` | 文本、相邻 `Span style="…"`、`br`；保留作者换行并自动折行 |
| `Image` | `src` 为 Asset ID 或相对路径；PNG/JPEG；contain / cover / fill；边框、背景放在外层 Box；不支持图片圆角 |
| `Shape` | `shape="rect|ellipse|line"`，默认 rect |
| `Path` | `d` 为绝对 M/L/C/Z；`viewBox="100 100"` 为逻辑宽高；不是完整 SVG |
| `Table` | `Row` 内 `Cell`，矩形纯文字数据；`columns="2 1"` 为列宽比例；等高行、固定表头配色和边框，无合并单元格 |
| `Chart` | `type="bar|line"`；`Labels` 用 `|` 分隔；`Series name="…" color="#…"` 内空格分隔有限数值；最多 8 系列、100 分类 |

表格和图表同时支持本地预览与原生 PPTX。图表导出缓存数值以及嵌入的 `.xlsx` 工作簿。当前 Office 图表使用自身坐标轴/图例排版，与本地预览的几何不保证一致。

## 布局属性

全部盒子采用 border-box。默认 display 是 block。当前属性值中不支持 calc、viewport 单位、rem/em、完整 minmax/repeat、媒体查询、自动分页、阴影、复杂滤镜和动画。

| 属性组 | 支持 |
|---|---|
| 尺寸 | width/height: px/pt/emu、%、auto；min/max-width/height |
| 盒模型 | padding / margin 的 1–4 值与四边属性；margin 允许 auto |
| 定位 | position: relative / absolute；left/top/right/bottom |
| Flex | display:flex；row / column；flex-grow、flex-shrink、flex-basis；gap；align-items/self；justify-content |
| Grid | 显式 px/pt/emu、正数 fr、auto 轨道；grid-template-columns/rows；gap；grid-column/row 的 line、span、`start / end` |
| 填充 | background/background-color：纯色或 `linear-gradient(to right|to bottom, #色1, #色2)`；渐变限两个不透明色标 |
| 边线 | `border:1px solid #D0D8DE`，或统一 border-width 与 border-color；非负 border-radius |
| 透明度 | opacity:0–1，仅叶子绘图对象；Table/Chart/容器不接受 opacity |
| 图片 | object-fit:contain / cover / fill |
| 文字 | font-family、font-size、font-weight:400/700、font-style:normal/italic、line-height、color、text-align:left/center/right/justify |
| 换行 | white-space:normal / nowrap / pre-wrap；当前 normal 与 pre-wrap 都保留 XML 文本中的空白，不实现浏览器空白折叠 |

字体必须真的包含所请求的字重/斜体 face；不合成。`Span` 只接受 font-family/size/weight/style、line-height 和 color。颜色、字重、字体继承；行高随字号按继承比率变化，显式 line-height 可用长度或倍数。布局使用 f32 并关闭 Taffy 像素取整；PPTX 到最后一步才转换整数 EMU。

完全空的 Text/Span 内容不产生行框，文字测量为 0×0、0 行；显式尺寸、背景和边框仍按盒子布局保留。明确输入的空格或换行属于内容，仍可占据行距。PPTX 导入中的纯空文本容器不会把线条/形状转成文字，空占位符也不会凭空增加一行。

`check` 报告文本溢出、页面越界，以及缩窄 3% 后换行高度超过容器的紧排版警告。错误阻止渲染和导出。目前不对所有物体重叠作审美或遮挡判定，尚未实现自动区域冻结和碰撞修复。装饰性重叠由作者自行设计。

0.2.0 导出 PPTX 时，未指定宽度或宽度 auto、采用内在宽度且没有软折行的紧标签禁用软折行，避免接收端在末尾拆出单个字母；显式宽度正文维持换行。`white-space:nowrap` 也输出禁用软折行。已有显式换行不会因此删除。这项策略不能保证所有 Office 渲染器的字形和文字边界相同。

窄文本可先通过 CLI `measure` 或 WASM `measureText` 获取实际字体的宽高与行数。推荐列表符号使用自动宽度，正文用 Flex 分配剩余空间，参考 [组件示例](../examples/components.xml)。这些 API 不改变语言 v1，详见 [渲染说明](RENDERING-0.2.0.md)。

## 导入边界

PPTX 限压缩 64 MiB、4096 个条目、总展开 256 MiB、单 XML 16 MiB。拒绝 ZIP 路径穿越、重复条目、符号链接、DTD 与外部实体；外部关系只记录，不获取。图片、字体与渲染表面有独立大小限制。这些是资源预算，不是完整的恶意 Office 文件安全审计。

导入重建文本、常用形状、受限自定义路径、图片、基本表格，以及有数值缓存的柱状/折线图；使用布局/母版定位占位符，现已解析主题色、颜色映射、常用变换、透明度与默认文字级联；完整主题效果、复杂段落和母版装饰仍有限。原生组几何折算后展开，不恢复组件。字体/段落/单元格的复杂局部格式、继承装饰、主题色、渐变、动画、批注、备注、媒体和其他扩展不能保证恢复。

`losses.json` 是已经识别的损失；原生对象完整结构摘要与整包部件清单还用于保守比较。没有列出损失不等于任意文件通过视觉等价认证。导入后的新导出必须检查旁车比较结果，不支持把“再次编译成功”解释为无损往返。

主题导入的准确支持范围、验证与限制见 [0.3.0 导入说明](OOXML-IMPORT-0.3.0.md)。
