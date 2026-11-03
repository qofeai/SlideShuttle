# SlideShuttle 0.3.0：主题、透明度与默认文字样式导入

本次针对 `schemeClr`、`alpha` 和 `defaultTextStyle` 补齐常用解析链路。结果进入场景和实际 PNG/PDF 渲染；原 PPTX 的结构摘要、主题部件与默认样式仍保留在比较清单中。源语言仍为 v1，引擎版本更新为 0.3.0，让旧旁车能够识别行为变更。

## 支持范围

| 范围 | 实现 |
|---|---|
| 关系与主题 | 按 relationship Type 读取主题/覆盖/版式/母版，不依赖固定文件名；clrScheme、clrMap、clrMapOvr 和 masterClrMapping |
| 样式引用 | fillRef、lnRef、bgRef；格式矩阵填充/线条列表；phClr 占位颜色 |
| 颜色来源 | sRGB、主题色、系统色 lastClr 回退、scRGB、HSL、可识别的预设色 |
| 颜色变换 | 按顺序处理 alpha/alphaMod/alphaOff、tint/shade、亮度/饱和度/色相和通道变换；最终量化为 RGBA |
| 透明度 | 分别保留填充、边线、文字的 alpha；图片 alphaModFix 转为 opacity |
| 文字继承 | presentation 默认样式 → 母版 title/body/other → 匹配占位符/版式 → 对象 lstStyle → 段落 defRPr → 局部 rPr |
| 文字属性 | 字体/主题字体、字号、颜色、400/700 字重、斜体、统一对齐、固定/百分比行距；b=0 / i=0 覆盖上层设置 |
| 背景 | 页面、版式、母版的背景回退；纯色和背景样式引用 |

支持的值展开为本项目的显式样式。本次没有创建可编辑的 Master/Layout 模型。未知主题色或变换仍报告具体损失，不能因清除了通用警告就算解析成功。tint/shade 和 RGB 通道变换在线性颜色空间计算，避免直接混合显示 RGB 产生明显偏色。

## 已执行验证

- 45 项 Rust 集成测试通过，新增 9 项覆盖颜色、半透明叠色、变换顺序、各层覆盖、样式矩阵、显式关闭粗斜体、占位符、非标准主题路径、主题覆盖、背景/图片透明度和未知值提示。
- 严格 Clippy、格式检查、release WASM 构建及原有空文本/字体/布局回归通过。
- Chromium + Worker 实际打开三页主题 PPTX：三页 PNG 与独立编写的显式颜色和样式 XML 参考稿逐字节一致，样例导入损失为 0；原有浏览器检查继续通过。
- LibreOffice 26.2.6.3 独立渲染：9 个背景/色块点最大 RGB 通道差 2/255；标题 30pt 粗体、正文 24pt 常规和局部 18pt 常规通过 PDF 字体/字号检查。保留 `allWithinOneChannelStep: false`，不将结果描述为 Office 像素一致。

浏览器样例/结果在 `examples/output/browser-v2/`：`theme-regression.pptx`、`theme-expected.xml`、`theme-native/`、`theme-preview-1.png` 等和 `report.json`。独立 PDF、PNG 与报告在 `examples/output/theme-office/`。

输入由 [create_theme_fixture.py](../tools/create_theme_fixture.py) 使用本项目自有内容生成。浏览器脚本自动生成样例、渲染显式样式参考稿并逐页比较 Worker 输出，已加入 CI，尚未运行远程 CI。独立比较使用 PyMuPDF、Pillow 和 [verify_theme_office.py](../tools/verify_theme_office.py)：

```bash
python tools/verify_theme_office.py \
  --pdf examples/output/theme-office/theme-regression.pdf \
  --native examples/output/browser-v2/theme-native \
  --out examples/output/theme-office
```

## 仍有的边界

1. Windows PowerPoint / WPS 尚未验收；独立应用证据来自 LibreOffice。
2. 探索样例中，仅在版式空占位符的 pPr/defRPr 设置字号时，LibreOffice 与本项目的回退策略不同。交叉验证样例使用对应级别的 lstStyle；pPr 级继承仍需 PowerPoint/WPS 核对。
3. 无界面运行时使用 sysClr 的 lastClr 作为确定性回退。Office 能读取操作系统实时系统色时，结果可能不同。
4. 非零缩进、项目符号、段前段后间距、一个文本对象内不同段落对齐，以及复杂表格单元格主题外观，继续给出具体限制。
5. 未识别的预设色、损坏的主题引用和未知变换保留损失。渐变、阴影、动画、完整母版装饰和任意 PPTX 无损往返仍未完成。
6. 用户提供的是限制日志，尚未取得其实际五页文稿；不声称已核对该文件的全部外观。

## 规则来源

参考 Microsoft 的 [ColorMapOverride](https://learn.microsoft.com/en-us/office/open-xml/presentation/working-with-slide-layouts)、[TextStyles](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.presentation.textstyles?view=openxml-3.0.1)、[DefaultTextStyle](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.presentation.defaulttextstyle?view=openxml-3.0.1)、[FillReference](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.fillreference?view=openxml-3.0.1) 和 [DrawingML Primer](https://download.microsoft.com/download/e/1/4/e14fb96f-83b8-4a2a-84db-7fa8acbe061a/Office%20Open%20XML%20Part%203%20-%20Primer.pdf)。线性颜色空间与独立结果差异另核对 [LibreOffice 官方实现](https://github.com/LibreOffice/core/blob/master/oox/source/drawingml/color.cxx)。本项目自行编写 Rust 实现，未复制 Kimi 私有格式或代码。
