# SlideShuttle 公开 benchmark 首轮实测

日期：2026-11-02。测试固定使用 SlideShuttle 0.1.0，生成模型为 Rex 交互会话中的 `claude-opus-5-5`。结论：12 个公开任务首轮 7 个完成；一次编译器反馈修复后全部完成。独立 PPTX 渲染仍暴露标签换行缺陷，实际母版继承也缺失，当前不能据此声称达到成熟演示生成产品水平。

## 选取哪些公开评测

| 评测 | 适用目标 | 本轮处理 |
|---|---|---|
| [SlidesBench / AutoPresent](https://github.com/para-lost/AutoPresent) | 单页指令到演示文稿，支持参照式和无参照评测 | 实际运行其公开 Detailed Instructions Only 任务的 12 条子集 |
| [SlidesGen-Bench](https://github.com/YunqiaoYang/SlidesGen-Bench) | 内容、计算视觉指标、原生可编辑深度 | 运行固定提交的计算视觉代码；另按 PEI 公开门槛做结构审计 |
| [PresentBench](https://github.com/PresentBench/PresentBench) | 整套演示、材料忠实度和任务专属 checklist | 核对公开数据与评测流程；本轮未运行其完整独立 VLM judge，未填写官方分数 |

SlidesBench 包含多种输入场景。本轮为隔离语言与编排能力，预先筛选不要求外部图片的任务：公开文件中有 64 条符合，按领域取数字排序中位页，再取剩余最长的三条，得到 12 条、9 个领域。艺术领域没有符合此筛选的任务。密集文本被刻意纳入，领域权重不均衡；**不是完整测试集的随机抽样，也不能代表图片生成或完整中文任务能力**。

固定提交与每条 instruction 的 SHA-256 见 [manifest](../benchmarks/slidesbench-pilot-manifest.json)。AutoPresent：`98e0c012e89469863d9c3c8bc87eac967d82b2e6`；SlidesGen-Bench：`b202e60c39e19c5f9da57029fe53ff189e6642b0`。

## 实际执行结果

| 测量项 | 首轮 | 仅一次诊断修复后 |
|---|---:|---:|
| 原生布局检查通过 | 7 / 12，58.3% | 12 / 12，100% |
| PNG/PDF 生成与 PPTX 导出成功 | 7 / 12 | 12 / 12 |
| 导出的 PPTX 可由 LibreOffice 打开并渲染 | 7 / 12 | 12 / 12 |
| 成功交付页面保留的指定引号字符串 / 全部任务要求 | 27 / 75 | 75 / 75 |
| 原生 Text 矩形的潜在重叠候选 | 成功页为 0 | 12 页为 0 |

引号字符串覆盖只是精确文字诊断，不是事实准确率、语义忠实度或可读性分数。上游文字存在重复、拼写和事实问题，本轮按任务保留，没有将其作为已核实事实。重叠检查也不是完整视觉遮挡判定。

生成首轮约 315.8 秒，接口记录输出 37,120 token，其中 thinking 17,183；修复约 99.5 秒。会话保留了此前架构讨论上下文，未进行冷启动、等预算模型横向对照，不能将这些时间/token 直接与论文系统比较。完整 usage 保留在原始 generation.json。

首轮失败的 5 页为 `career-slide_14`、`entrepreneur-slide_5`、`technology-slide_9`、`career-slide_15`、`career-slide_16`。错误都集中在独立列表符号的 Text 节点：模型给符号分配的宽度小于字体测量所需宽度。修复扩大了符号空间、调整相邻正文宽度和 gap，保留全部文字；字号没有作为此次修复的缩小手段。其余 7 页保持原样，机器合并原始回复和修复回复，未进行人工布局修改。

## 最终 PPTX 的独立视觉测量

输出经 **LibreOffice 26.2.6.3** 转为 PDF，再以 1280×720 栅格化。容器 fontconfig 确认选择的是与原生端相同的 `Noto Sans CJK SC` 字体文件。以下数值来自最终 PPTX 的渲染，不取自自身预览。

计算直接运行上游 `aesthetics_metrics.py` 与参数文件，代码 SHA-256 为 `7f3179540e603526c04669226e5804ca1b8aa3476996033b75440c2751fecf48`。唯一输入适配是用 PDF 的实际文字边界替代 Paddle 版面检测。

| 原始指标 | 首轮成功的 7 页均值 | 修复后 12 页均值 | 解读边界 |
|---|---:|---:|---|
| 文字对比度，归一化 0–1 | 0.7596 | 0.7088 | 反映文字区域的图地对比，不检测错误换行 |
| 色彩调和距离 | 0.01009 | 0.01188 | 距离较小表示更接近色彩模板，不等于更美观 |
| 色彩丰富度 | 76.1060 | 55.4932 | 原始色彩量，不是越大越好 |
| 子带熵 | 3.4420 | 3.5382 | 视觉复杂度，不能单独判断设计好坏 |

两列样本数不同，不能解释为“修复让审美下降”。12 个无关单页任务不构成连续叙事，因此没有计算跨页节奏、全套审美总分或市场排行榜名次。

作为校验，纯白空页的调和距离和子带熵都为 0，但完全没有任务内容。该对照说明这些视觉代理指标必须结合内容和人工/独立视觉判断使用，不能把单个指标当审美总分。

## 发现的实际缺口

### 1. 原生预览通过，导出后仍可能错误换行

`design-slide_5` 原生预览中 Arrangement、Proximity、Repetition、Contrast、Balance 均为单行；独立渲染后末尾字母被换到下一行。其标签采用无显式宽度的紧包文本框，表明不同排版器在紧边界处仍有差异。

- [原生预览](../benchmarks/runs/2026-11-02-slidesbench-pilot/attempt-1/design-slide_5/native/slide-001.png)
- [最终 PPTX 的 LibreOffice 渲染](../benchmarks/runs/2026-11-02-slidesbench-pilot/attempt-1/design-slide_5/libreoffice/slide-001.png)

该页仍然满足本轮两个“引号字符串”要求，因为五个标签并未被上游指令加引号。另行逐词诊断捕获了这五个断裂单词。**所以 75/75 字符串覆盖和 12/12 编译通过都不能证明最终视觉正确。**

下一步应处理文本框的换行策略、内在尺寸与导出容差，并把相同 XML 在原生端和独立渲染端的差异加入回归。LibreOffice 中发现的问题不直接等同于 PowerPoint/WPS 行为，后两者仍需实测。

### 2. 列表符号需要类型化组件或测量支持

一个重复的列表符号尺寸问题导致首轮 5 页失败。值得优先提供自动测量的 Bullet/List 组件，并让 Agent 获取实际文本测量，而不依赖估算每个窄文本框。现有诊断有效帮助模型在一次反馈中修复，但首轮成功率仍需提高。

### 3. 不能因存在空母版和原生图表而声称深层可编辑

按 [PEI 的公开 knockout 规则](https://github.com/YunqiaoYang/SlidesGen-Bench/blob/main/docs/pei.md)，L3 要求实际母版继承与逻辑组织。当前输出有母版文件，但母版里没有实际绘制对象，内容仍逐页放置；本地审计因此将其能力上限保守限制在 L2，**不能主张已达到 L3/L4**。这不是官方 PEI 评分器给出的等级。

已有原生图表和工作簿是有效能力，但在 L3 未满足时不能跳级。后续需要在语言、原生预览和 PPTX 输出中共同实现 Master/Layout 继承。

### 4. 真实外部 PPTX 的导入范围仍有限

从 SlidesGen-Bench 的公开 `Quake/topic_introduction/modern_architecture` 目录下载了一份约 3.2 MB 的 PPTX。读取到 **31 页、731 个对象，报告 1,391 条重建限制记录**，涉及主题色、旋转/翻转、透明度、效果、渐变、分组、母版装饰和连接线等。

限制记录会重复覆盖同一对象，不能用 1,391 / 731 算对象丢失率。本轮完成的是结构检查，没有据此宣称这份外部文件能够被忠实还原。

## 产物与复现

- [12 页 PPTX](../benchmarks/runs/2026-11-02-slidesbench-pilot/final/SlideShuttle-SlidesBench-12.pptx)
- [独立渲染 PDF](../benchmarks/runs/2026-11-02-slidesbench-pilot/final/libreoffice/SlideShuttle-SlidesBench-12.pdf)
- [12 页预览总览](../benchmarks/runs/2026-11-02-slidesbench-pilot/attempt-2/contact-sheet.png)
- [机器结果摘要](../benchmarks/runs/2026-11-02-slidesbench-pilot/summary.json)
- [复现脚本与限制](../benchmarks/README.md)

源代码、CLI 二进制、Cargo.lock、字体和容器 image digest 均记录在 run 的 environment.json。原始 prompt、回复、首轮失败、修复反馈、每页源码、检查日志、PPTX、图像、指标全部保留在忽略版本控制的 runs 目录。输入准备脚本和四项评测工具测试已执行。

本轮**没有运行完整 SlidesBench 585 条测试、PresentBench 官方 judge、SlidesGen QuizBank 内容考试或四组等预算语言对照**。官方排行榜分数明确为 null；不能与 NotebookLM、Kimi、Gamma 等公开榜单数字直接比较。

## 后续渲染回归：0.2.0

同日针对上述问题完成实现后的复测，保留前文 0.1.0 的原始结果。使用 `attempt-2/raw-deck.xml` 原样输入，SHA-256 为 `23c5705761f62e1b17178b6b14f3a17e8b89164a0438a88c83b389db47de2057`，不调用模型、不修改文字或布局。

导出器为自动内在宽度、未软折行的紧标签输出 `wrap="none"`；固定宽度正文仍保留软折行策略，显式 nowrap 同样保留。12 页经 LibreOffice 26.2.6.3 独立渲染，`design-slide_5` 的五个标签均恢复单行。PDF 中四字符及以上单词与源文件的归一化集合比较，12 页均无缺失/拆词记录。集合比较不验证顺序、重复次数和整体视觉，不替代人工核对；本轮没有重跑审美分数或官方 judge。

- [修复后独立渲染总览](../examples/output/rendering-v2/contact-sheet.png)
- [已知标签问题的页面](../examples/output/rendering-v2/slide-003.png)
- [机器结果与源码摘要](../examples/output/rendering-v2/report.json)
- [冻结源码回归工具](../tools/render_regression.py)

列表相关的改进是可复用的真实文字测量 API 与自动占位 `BulletRow` 组件示例，尚未重新生成这 12 个任务测量首轮成功率。Master/Layout 继承、PEI 深度缺口和复杂外部 PPTX 导入限制仍在。
