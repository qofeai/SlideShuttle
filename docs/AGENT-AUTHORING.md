# 给创作 Agent 的操作约定

你在生成 SlideShuttle 多文件静态 XML 演示工程（打包为 `.qppt`）。先读 [LANGUAGE.md](LANGUAGE.md) 与 [slideshuttle Skill](../skills/slideshuttle/SKILL.md)，只使用能力表中的属性和值。不要生成 JavaScript、React 运行时代码、浏览器 API 或依赖完整 CSS 的效果。

1. **先做减法，严格区分“画面”与“讲稿”**：每页 Slide 只解决一个判断、突出一个视觉焦点（大数字、图表、核心公式或关系图）。画面可见文字控制在 **60–150 字（上限 200 字）**，使用短语而非长句；所有口语解释、完整推导和背景故事一律写进 `<Notes>`。需要讲更多细节时**增加页数**，绝不把一页塞成密集讲义。
2. **严守字号与留白底线（1280×720）**：主标题 `40–56px`、副标题 `26–32px`、正文/要点 **`20–26px`（严禁 `< 20px`）**、图注/页脚 **`16–18px`（严禁 `< 16px`）**。画布留白率 **≥ 40%**，单页渲染节点 **≤ 20 个**，拒绝“大框套中框套小框”的冗余边框与多色底框。
3. **语义稳定 ID 与组件复用**：按 `deck.xml` + `design.xml` + `pages/*.xml` 组织，元素 ID 仅需页内唯一。窄标签先用 `slide-shuttle measure` 测量。数学上下标使用 `<Span style="vertical-align:sub/super">`，避免使用字体缺失的生僻 Unicode 符号。
4. **原生对象优先**：数据用 `Table` / `Chart`；图片用 `Image`；单色图标用 `Icon`；复合图示用一层 `Group`。不要用整页截图冒充原生可编辑文稿。
5. **构建、美学审计与看图复核**：运行 `python3 skills/slideshuttle/scripts/build.py <deck.xml> --out <out_dir>`，同时清零编译 `diagnostics` 与美学密度审计 `designAudit` 警告；随后必须逐页查看渲染出的 PNG，确认视觉主次、留白与断行自然。
6. **已有外部稿保护**：处理导入稿时提供基线和 `--against`，读取 `diff` / `losses`，用绑定当前 `diffId` 的决策文件记录明确授权的有损重建，不绕过旁车保护。
