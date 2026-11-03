# 预设形状

`<Shape shape="…">` 接受 OOXML 的全部 187 种预设几何，几何直接按 ECMA-376 `presetShapeDefinitions.xml` 的公式计算（与 PowerPoint 同源），PPTX 中写为原生 `prstGeom`，导入时原样还原。本表由 `slide-shuttle shapes` 生成，Agent 可直接调用该命令查询。

`adjust` 按下表顺序给出空格分隔的数值，可只写前几个，其余取默认值。数值是 DrawingML 原始单位：大多数为短边或宽高的 1/100000（50000 = 50%），角度为 1/60000 度。不确定时先用默认值，看图后再微调。

- `line` 与名称含 `Connector` 的连接线、`arc`、括号类可用 `marker-start` / `marker-end`；它们的 `border` 是描边，不占布局空间。
- `rect` 用 `border-radius` 做圆角（导出为 `roundRect`）；`ellipse`、`line` 与其余预设一样直接使用。
- 部分预设（如 can、cube、bevel）带有自动加深或提亮的面，用同一个 background 着色。

| 预设 | adjust 默认值 |
|---|---|
| `accentBorderCallout1` | adj1=18750 adj2=-8333 adj3=112500 adj4=-38333 |
| `accentBorderCallout2` | adj1=18750 adj2=-8333 adj3=18750 adj4=-16667 adj5=112500 adj6=-46667 |
| `accentBorderCallout3` | adj1=18750 adj2=-8333 adj3=18750 adj4=-16667 adj5=100000 adj6=-16667 adj7=112963 adj8=-8333 |
| `accentCallout1` | adj1=18750 adj2=-8333 adj3=112500 adj4=-38333 |
| `accentCallout2` | adj1=18750 adj2=-8333 adj3=18750 adj4=-16667 adj5=112500 adj6=-46667 |
| `accentCallout3` | adj1=18750 adj2=-8333 adj3=18750 adj4=-16667 adj5=100000 adj6=-16667 adj7=112963 adj8=-8333 |
| `actionButtonBackPrevious` | — |
| `actionButtonBeginning` | — |
| `actionButtonBlank` | — |
| `actionButtonDocument` | — |
| `actionButtonEnd` | — |
| `actionButtonForwardNext` | — |
| `actionButtonHelp` | — |
| `actionButtonHome` | — |
| `actionButtonInformation` | — |
| `actionButtonMovie` | — |
| `actionButtonReturn` | — |
| `actionButtonSound` | — |
| `arc` | adj1=1.62e+07 adj2=0 |
| `bentArrow` | adj1=25000 adj2=25000 adj3=25000 adj4=43750 |
| `bentConnector2` | — |
| `bentConnector3` | adj1=50000 |
| `bentConnector4` | adj1=50000 adj2=50000 |
| `bentConnector5` | adj1=50000 adj2=50000 adj3=50000 |
| `bentUpArrow` | adj1=25000 adj2=25000 adj3=25000 |
| `bevel` | adj=12500 |
| `blockArc` | adj1=1.08e+07 adj2=0 adj3=25000 |
| `borderCallout1` | adj1=18750 adj2=-8333 adj3=112500 adj4=-38333 |
| `borderCallout2` | adj1=18750 adj2=-8333 adj3=18750 adj4=-16667 adj5=112500 adj6=-46667 |
| `borderCallout3` | adj1=18750 adj2=-8333 adj3=18750 adj4=-16667 adj5=100000 adj6=-16667 adj7=112963 adj8=-8333 |
| `bracePair` | adj=8333 |
| `bracketPair` | adj=16667 |
| `callout1` | adj1=18750 adj2=-8333 adj3=112500 adj4=-38333 |
| `callout2` | adj1=18750 adj2=-8333 adj3=18750 adj4=-16667 adj5=112500 adj6=-46667 |
| `callout3` | adj1=18750 adj2=-8333 adj3=18750 adj4=-16667 adj5=100000 adj6=-16667 adj7=112963 adj8=-8333 |
| `can` | adj=25000 |
| `chartPlus` | — |
| `chartStar` | — |
| `chartX` | — |
| `chevron` | adj=50000 |
| `chord` | adj1=2.7e+06 adj2=1.62e+07 |
| `circularArrow` | adj1=12500 adj2=1.14232e+06 adj3=2.04577e+07 adj4=1.08e+07 adj5=12500 |
| `cloud` | — |
| `cloudCallout` | adj1=-20833 adj2=62500 |
| `corner` | adj1=50000 adj2=50000 |
| `cornerTabs` | — |
| `cube` | adj=25000 |
| `curvedConnector2` | — |
| `curvedConnector3` | adj1=50000 |
| `curvedConnector4` | adj1=50000 adj2=50000 |
| `curvedConnector5` | adj1=50000 adj2=50000 adj3=50000 |
| `curvedDownArrow` | adj1=25000 adj2=50000 adj3=25000 |
| `curvedLeftArrow` | adj1=25000 adj2=50000 adj3=25000 |
| `curvedRightArrow` | adj1=25000 adj2=50000 adj3=25000 |
| `curvedUpArrow` | adj1=25000 adj2=50000 adj3=25000 |
| `decagon` | vf=105146 |
| `diagStripe` | adj=50000 |
| `diamond` | — |
| `dodecagon` | — |
| `donut` | adj=25000 |
| `doubleWave` | adj1=6250 adj2=0 |
| `downArrow` | adj1=50000 adj2=50000 |
| `downArrowCallout` | adj1=25000 adj2=25000 adj3=25000 adj4=64977 |
| `ellipse` | — |
| `ellipseRibbon` | adj1=25000 adj2=50000 adj3=12500 |
| `ellipseRibbon2` | adj1=25000 adj2=50000 adj3=12500 |
| `flowChartAlternateProcess` | — |
| `flowChartCollate` | — |
| `flowChartConnector` | — |
| `flowChartDecision` | — |
| `flowChartDelay` | — |
| `flowChartDisplay` | — |
| `flowChartDocument` | — |
| `flowChartExtract` | — |
| `flowChartInputOutput` | — |
| `flowChartInternalStorage` | — |
| `flowChartMagneticDisk` | — |
| `flowChartMagneticDrum` | — |
| `flowChartMagneticTape` | — |
| `flowChartManualInput` | — |
| `flowChartManualOperation` | — |
| `flowChartMerge` | — |
| `flowChartMultidocument` | — |
| `flowChartOfflineStorage` | — |
| `flowChartOffpageConnector` | — |
| `flowChartOnlineStorage` | — |
| `flowChartOr` | — |
| `flowChartPredefinedProcess` | — |
| `flowChartPreparation` | — |
| `flowChartProcess` | — |
| `flowChartPunchedCard` | — |
| `flowChartPunchedTape` | — |
| `flowChartSort` | — |
| `flowChartSummingJunction` | — |
| `flowChartTerminator` | — |
| `foldedCorner` | adj=16667 |
| `frame` | adj1=12500 |
| `funnel` | — |
| `gear6` | adj1=15000 adj2=3526 |
| `gear9` | adj1=10000 adj2=1763 |
| `halfFrame` | adj1=33333 adj2=33333 |
| `heart` | — |
| `heptagon` | hf=102572 vf=105210 |
| `hexagon` | adj=25000 vf=115470 |
| `homePlate` | adj=50000 |
| `horizontalScroll` | adj=12500 |
| `irregularSeal1` | — |
| `irregularSeal2` | — |
| `leftArrow` | adj1=50000 adj2=50000 |
| `leftArrowCallout` | adj1=25000 adj2=25000 adj3=25000 adj4=64977 |
| `leftBrace` | adj1=8333 adj2=50000 |
| `leftBracket` | adj=8333 |
| `leftCircularArrow` | adj1=12500 adj2=-1.14232e+06 adj3=1.14232e+06 adj4=1.08e+07 adj5=12500 |
| `leftRightArrow` | adj1=50000 adj2=50000 |
| `leftRightArrowCallout` | adj1=25000 adj2=25000 adj3=25000 adj4=48123 |
| `leftRightCircularArrow` | adj1=12500 adj2=1.14232e+06 adj3=2.04577e+07 adj4=1.19423e+07 adj5=12500 |
| `leftRightRibbon` | adj1=50000 adj2=50000 adj3=16667 |
| `leftRightUpArrow` | adj1=25000 adj2=25000 adj3=25000 |
| `leftUpArrow` | adj1=25000 adj2=25000 adj3=25000 |
| `lightningBolt` | — |
| `line` | — |
| `lineInv` | — |
| `mathDivide` | adj1=23520 adj2=5880 adj3=11760 |
| `mathEqual` | adj1=23520 adj2=11760 |
| `mathMinus` | adj1=23520 |
| `mathMultiply` | adj1=23520 |
| `mathNotEqual` | adj1=23520 adj2=6.6e+06 adj3=11760 |
| `mathPlus` | adj1=23520 |
| `moon` | adj=50000 |
| `noSmoking` | adj=18750 |
| `nonIsoscelesTrapezoid` | adj1=25000 adj2=25000 |
| `notchedRightArrow` | adj1=50000 adj2=50000 |
| `octagon` | adj=29289 |
| `parallelogram` | adj=25000 |
| `pentagon` | hf=105146 vf=110557 |
| `pie` | adj1=0 adj2=1.62e+07 |
| `pieWedge` | — |
| `plaque` | adj=16667 |
| `plaqueTabs` | — |
| `plus` | adj=25000 |
| `quadArrow` | adj1=22500 adj2=22500 adj3=22500 |
| `quadArrowCallout` | adj1=18515 adj2=18515 adj3=18515 adj4=48123 |
| `rect` | — |
| `ribbon` | adj1=16667 adj2=50000 |
| `ribbon2` | adj1=16667 adj2=50000 |
| `rightArrow` | adj1=50000 adj2=50000 |
| `rightArrowCallout` | adj1=25000 adj2=25000 adj3=25000 adj4=64977 |
| `rightBrace` | adj1=8333 adj2=50000 |
| `rightBracket` | adj=8333 |
| `round1Rect` | adj=16667 |
| `round2DiagRect` | adj1=16667 adj2=0 |
| `round2SameRect` | adj1=16667 adj2=0 |
| `roundRect` | adj=16667 |
| `rtTriangle` | — |
| `smileyFace` | adj=4653 |
| `snip1Rect` | adj=16667 |
| `snip2DiagRect` | adj1=0 adj2=16667 |
| `snip2SameRect` | adj1=16667 adj2=0 |
| `snipRoundRect` | adj1=16667 adj2=16667 |
| `squareTabs` | — |
| `star10` | adj=42533 hf=105146 |
| `star12` | adj=37500 |
| `star16` | adj=37500 |
| `star24` | adj=37500 |
| `star32` | adj=37500 |
| `star4` | adj=12500 |
| `star5` | adj=19098 hf=105146 vf=110557 |
| `star6` | adj=28868 hf=115470 |
| `star7` | adj=34601 hf=102572 vf=105210 |
| `star8` | adj=37500 |
| `straightConnector1` | — |
| `stripedRightArrow` | adj1=50000 adj2=50000 |
| `sun` | adj=25000 |
| `swooshArrow` | adj1=25000 adj2=16667 |
| `teardrop` | adj=100000 |
| `trapezoid` | adj=25000 |
| `triangle` | adj=50000 |
| `upArrow` | adj1=50000 adj2=50000 |
| `upArrowCallout` | adj1=25000 adj2=25000 adj3=25000 adj4=64977 |
| `upDownArrow` | adj1=50000 adj2=50000 |
| `upDownArrowCallout` | adj1=25000 adj2=25000 adj3=25000 adj4=48123 |
| `uturnArrow` | adj1=25000 adj2=25000 adj3=25000 adj4=43750 adj5=75000 |
| `verticalScroll` | adj=12500 |
| `wave` | adj1=12500 adj2=0 |
| `wedgeEllipseCallout` | adj1=-20833 adj2=62500 |
| `wedgeRectCallout` | adj1=-20833 adj2=62500 |
| `wedgeRoundRectCallout` | adj1=-20833 adj2=62500 adj3=16667 |

几何数据来源：ECMA-376 第 1 部分附带的 presetShapeDefinitions.xml（经 Apache POI 分发），由 `tools/build_presets.py` 转换为 `crates/slide-shuttle-core/data/presets.txt`。
