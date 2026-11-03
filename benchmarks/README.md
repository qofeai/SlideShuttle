# Public slide benchmark pilot

本目录提供公开数据准备、模型调用记录、逐页执行与指标脚本。首轮结果见 [BENCHMARK-2026-11-02.md](../docs/BENCHMARK-2026-11-02.md)。它是明确改造范围的小样本测试，**不生成官方排行榜分数**。

输入来自 [AutoPresent / SlidesBench](https://github.com/para-lost/AutoPresent)，任务选择固定在 [manifest](slidesbench-pilot-manifest.json)：所有 `instruction_no_image.txt` 中筛选不要求 image/photo/picture/logo/icon 的任务，按领域取数字排序中位页，再加入剩余最长的三条。64 条符合筛选条件，最终 12 条覆盖 9 个领域；art_photos 无符合任务。筛选发生在生成之前。

这会排除图片生成场景，也不能代表完整 585 条测试集。保留上游提供的文字，包括重复、拼写和事实问题；本轮没有把任务文字当成经过核实的事实来源。

## 复现

从仓库根目录运行，下载只进入忽略版本控制的 cache/runs：

```bash
python3 -m venv target/bench-venv
target/bench-venv/bin/pip install -r benchmarks/requirements.txt
python3 benchmarks/prepare.py --run benchmarks/runs/my-run
```

用相同的语言说明和每条原始 instruction 让模型生成一份 12 页 `<Deck>`。保存完整 prompt、模型 ID、原始回复和 usage。实际首轮通过用户已授权的 Rex 交互式 Claude 生成，模型为 `claude-opus-5-5`，保留了先前架构讨论上下文；没有使用 `claude -p`。这不等同于冷启动、等预算的模型横向对比。

`rex_generate.py` 可向指定的现有交互会话发送 prompt，并监听完成事件。会话日志可能延迟落盘；遇到 capture 缺失时，从同一会话的原始 assistant 记录恢复，不能重新生成后冒充首轮。本次完整原始记录留在 run 目录。

把模型完整回复放进 `benchmarks/runs/my-run/attempt-1/response.md`，运行：

```bash
target/bench-venv/bin/python benchmarks/evaluate_pilot.py \
  --run benchmarks/runs/my-run \
  --font /path/to/NotoSansCJK-Regular.ttc \
  --font /path/to/NotoSansCJK-Bold.ttc
```

脚本逐页保留 source、check、PNG/PDF、PPTX 和失败，不删除失败页。原文引号内字符串的覆盖检查只是诊断，不是语义正确率或官方内容指标。

## 独立渲染与计算指标

实际测试使用临时 Ubuntu 26.04 容器的 **LibreOffice 26.2.6.3** 打开输出 PPTX，再把 PDF 栅格化。字体使用与原生端相同的 Noto 文件。`environment.json` 记录 image digest、源码、二进制、Cargo.lock 与字体摘要。没有用原生预览代替最终 PPTX 的独立渲染，也没有把 LibreOffice 结果当作 PowerPoint/WPS 认证。

准备包含 LibreOffice 的容器，挂载仓库到 `/work`，并使容器 fontconfig 可找到相同字体，之后运行：

```bash
target/bench-venv/bin/python benchmarks/render_and_score.py \
  --run benchmarks/runs/my-run --container MY_RENDERER
```

计算实现直接加载固定提交的 [SlidesGen-Bench](https://github.com/YunqiaoYang/SlidesGen-Bench) `aesthetics_metrics.py` 和参数文件，不修改其公式。适配点是：用 PDF 中实际文字边界替代 Paddle 版面检测。分别记录文字对比、色彩调和距离、色彩丰富度和子带熵。12 个独立任务不构成一套连续叙事稿，因此不计算跨页节奏或官方审美总分。

PEI 检查是针对公开 [PEI 定义](https://github.com/YunqiaoYang/SlidesGen-Bench/blob/main/docs/pei.md) 的本地审计。特别检查空母版，避免“包里存在母版文件”就认定具有实际母版继承。它不是上游提供的官方 PEI 评分器。

## 本次原始产物

- `runs/2026-11-02-slidesbench-pilot/attempt-1`：首轮模型回复、全部 12 条结果与 7 条成功产物。
- `repair-response`：只给失败页的编译器反馈，允许一次修复。
- `attempt-2`：机器合并修复的 5 页，其余 7 页保持不变；保留第二轮逐页结果。
- `final`：12 页合并 PPTX / PDF，供人工查看；它不是要求叙事连贯的原始整套生成任务。
- `external-import-summary.json`：SlidesGen-Bench 公开 31 页真实 PPTX 的结构读取与损失统计。

cache、runs、旁车和图片均不自动入 Git，原始材料依其上游许可使用。上游 AutoPresent 的代码采用 MIT；SlidesGen-Bench README 声明 MIT；PresentBench 的任务/量表标为 CC-BY-NC-4.0，原始素材另有来源许可，本轮没有将其作为生成输入或复制进产品。

```bash
TMPDIR="$PWD/target/tmp" python3 -m unittest discover -s benchmarks -p 'test_*.py'
```
