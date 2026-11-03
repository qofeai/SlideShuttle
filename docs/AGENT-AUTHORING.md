# 给创作 Agent 的操作约定

你在生成 SlideShuttle Language v1。先读 [LANGUAGE.md](LANGUAGE.md)，只使用能力表中的属性和值。不要生成 JavaScript、React 运行时代码、浏览器 API 或依赖完整 CSS 的效果。

1. 先确定内容事实、页级论点和顺序，再选择视觉表达。示例中的图表是演示数据，不能引用为性能结果。
2. 为整份稿建立一套 Token、文字层级、颜色和间距。优先用 Flex/Grid 表达关系，装饰和确实需要定位的区域可用 absolute。
3. 所有节点使用稳定语义 ID。复用布局用 Component / Use，不要在微小修改时把所有 ID 重建。
4. 显式提供字体文件与实际字重。字号和行高用支持的单位。保持文字量适中，给换行留空间。窄标签可先用 `slide-shuttle measure` 或 `Engine.measureText` 获取真实尺寸；列表符号采用内在宽度，正文分配剩余空间。可参考 `examples/components.xml` 的 `BulletRow`，无需硬编码符号宽度。
5. 数据用 Table / Chart；图片用 Image；需要在 PowerPoint 中整体移动的复合插画用一层 Group。不要用整页图片冒充原生编辑。
6. 每次修改运行 `slide-shuttle check`。根据节点 ID 和行列修复错误，再 render 并检查全部页面。读取 warning，确认紧排版是否需要更多空间。
7. 导出前做原生预览；需要验收 Office 时用 Office 出图，保留应用、字体与原始结果。两种预览的目标不同。
8. 处理已有外部稿时提供基线和 `--against`。读取 diff / losses；能采纳的改动写回源后重新比较。明确允许的有损重建用绑定当前 diff 的决策文件记录处理依据；语义冲突没有已知意图时集中询问。
9. 导出到新路径。只分享最终 PPTX/PDF/PNG；私有旁车、比较报告和决策可能含历史正文，不能自动作为收件人附件。

当前工具没有自动审美评分、自动区域冻结、自动碰撞修复、原包 patch 或任意 PPTX 无损回写。不要把多次编译成功作为这些能力的证据。

同一资源集反复改稿时复用 `Compiler` / `Engine`；每版生成 `CompiledDocument` / `CompiledDeck`，再逐页渲染。编译成功后仍需检查 `inspect().valid` 与诊断；无效场景可以读取诊断，但不能渲染或导出。浏览器批量操作优先在 Worker 内运行。文字测量与生命周期说明见 [渲染 API](RENDERING.md)。

可直接使用的创作 Skill 在 [skills/slideshuttle](../skills/slideshuttle/SKILL.md)，包含构图选择、可执行示例、构建脚本和渲染后复核流程。
