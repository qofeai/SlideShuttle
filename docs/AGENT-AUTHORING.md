# 给创作 Agent 的操作约定

创作流程、设计原则和审美红线都以 [slideshuttle Skill](../skills/slideshuttle/SKILL.md) 为准，这里不再重复。完整语法见 [LANGUAGE.md](LANGUAGE.md)，只使用其中列出的属性和值；不要生成 JavaScript、React 运行时代码或依赖完整 CSS 的效果。

处理导入的外部稿时，提供基线和 `--against`，读取 `diff` / `losses`；明确授权的有损重建，用绑定当前 `diffId` 的决策文件记录，不要绕过旁车保护。
