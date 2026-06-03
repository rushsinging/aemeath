# Prompt / Guidance

**Scope**：`agent/features/prompt/**`——Guidance 系统、系统提示构建、上下文注入。
**主触发**：改 `agent/features/prompt/**`。
**次触发**：改 provider 默认 model（影响 guidance 前缀匹配），或改系统提示注入。

## Guidance 系统

- 实现：`agent/features/prompt/src/business/guidance/`（+ `guidance.rs`）。
- Guidance 文件存放在 `~/.agents/guidance/`：
  - `_default.md` — 所有模型通用。
  - `{prefix}.md` — 按 model id 前缀匹配（**最长匹配优先**）。
  - `_reasoning.md` — reasoning 开启时附加。
- 首次运行自动生成默认文件，**不覆盖**用户编辑。
- 改 provider 默认 model 时注意：model id 变化会影响 `{prefix}.md` 的命中，需确认对应 guidance 仍匹配。
