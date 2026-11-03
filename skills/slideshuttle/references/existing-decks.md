# 已有稿件与导出边界

对已有 SlideShuttle 源稿保留稳定 ID、资源和旁车，沿着用户意图改动。`scripts/build.py` 会执行 CLI 原有保护，不会自动接受重建损失。

原始 PPTX 的导入是有限重建，通常变为绝对布局；不能恢复作者的组件/Grid 意图，也不保证任意文件无损。需要此模式时读取当前仓库 README 的“导入与继续改稿”以及 `docs/OOXML-IMPORT.md`。

```bash
slide-shuttle import original.pptx --out imported \
  --font /path/to/regular.ttf --font /path/to/bold.ttf

slide-shuttle diff imported/deck.xml --base BASELINE.json --against external.pptx \
  --font /path/to/regular.ttf --font /path/to/bold.ttf --out review.json
```

基线 B、外部稿 E、计划输出 P 的比较会报告冲突和未重建内容。没有 E 只表示 externalChecked:false，不能代表其他设备上的副本已检查。先读取损失与比较结果，按已有任务授权采纳改动；确实要舍弃某项内容时，决策必须绑定当前 diffId，并写该项的具体理由。意图不足以决定语义冲突时才询问。

不要复制到无基线目录、删除 `.slide-shuttle/` 或批量接受所有限制来绕过保护。原稿未表达的内容不会因为再次编译成功而变成无损。当前流程生成新 PPTX，不直接给原包打补丁，也不自动合并。

PPTX 目前只引用字体名称，PDF 嵌入子集字体；收件端查看 PPTX 需要相应字体。私有旁车与比较报告可能包含历史正文，不自动作为交付附件。
