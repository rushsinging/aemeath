# Feature #44：Commit Style Context 与 AI 协作者 trailer

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 完成日期 | 2026-05 |
| 归档日期 | 2026-05-24 |
| 状态 | 已确认完成 |

## 需求

在需要创建 git commit 时，LLM 应先分析当前仓库历史 Commit Style Context，优先查看带 `Co-Authored-By` 的提交，再按项目风格生成 commit message。AI 协作者 trailer 使用 `Co-Authored-By: Aemeath (<provider>/<model>) <github:rushsinging/aemeath>`，其中 provider/model 来自当前 LLM client。

## 实现结果

1. 已在 system prompt dynamic context 中加入 Commit Message Guidance。
2. Commit Message Guidance 要求 LLM 在创建 commit 前调用内置 `commit` skill。
3. 内置 `commit` skill 优先采样带 `Co-Authored-By` 的提交来分析当前仓库历史 commit 风格。
4. AI 协作者 trailer 使用 `Co-Authored-By: Aemeath (<provider>/<model>) <github:rushsinging/aemeath>`。
5. provider/model 来自当前 `LlmClient`。
6. 内置 `commit` skill 作为最低优先级 fallback 注册，项目/全局同名 skill 可覆盖。
7. skill 内容要求检查 `git status --short --branch`、采样 commit history、检查变更范围并执行 `git commit`。
8. 未在 session 初始化执行 git log，也未提前生成历史摘要。

## 设计文档

详细设计保留在 `docs/feature/specs/044-commit-style-context.md`。

## 验证

2026-05-24 用户确认 feature #44 已完成。活动列表中移除 #44，并保留此归档记录。
