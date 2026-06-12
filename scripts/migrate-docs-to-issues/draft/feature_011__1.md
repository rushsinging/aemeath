<!-- Migrated from: docs/feature/archived/011-openai-reasoning-effort.md -->
# #11 OpenAI reasoning_effort 配置支持

**归档日期**：2026-05-04

**确认结果**：用户确认完成

**目标**：支持 GPT-5.x / o 系列等 OpenAI reasoning 模型的 `reasoning_effort` 参数，让用户可通过配置控制 thinking 强度。

**实现**：
- 模型配置支持 `reasoning_effort`，取值 `none` / `low` / `medium` / `high` / `xhigh`。
- 配置加载阶段校验非法 effort，避免运行时才失败。
- OpenAI Compatible 请求体在支持 reasoning effort 的模型上透传 `reasoning_effort`。
- provider/model 能力检测区分支持与不支持 reasoning effort 的模型，避免向 GPT-4o 等模型发送无效字段。
- CLI/env/运行时配置链路可覆盖 reasoning effort，并与现有 reasoning 开关并存。

**说明**：
- OpenAI Chat Completions 默认不返回完整 thinking 文本；该 feature 仅控制 effort 参数，不承诺展示 OpenAI 内部 reasoning 内容。
- 后续如需 reasoning summary，可单独评估 Responses API 路径。

**涉及文件**：
- `aemeath-core/src/config/models.rs`
- `aemeath-core/src/provider.rs`
- `aemeath-llm/src/providers/openai_compatible/mod.rs`
- `aemeath-llm/src/providers/openai_compatible/non_stream.rs`
- `aemeath-llm/src/providers/openai_compatible/message_conversion.rs`
- `aemeath-cli/src/main.rs`
- `aemeath-cli/src/tui/status_bar.rs`
