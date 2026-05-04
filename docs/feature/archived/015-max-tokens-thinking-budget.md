# #15 通过 max_tokens 配置 LLM 输出 + thinking 双上限

**归档日期**：2026-05-04

**确认结果**：用户确认完成

**目标**：统一支持输出 token 上限与 thinking token 上限配置，覆盖 provider 默认、model 覆盖及运行时覆盖，并按 provider 能力映射到实际请求字段。

**实现**：
- 支持输出 token 上限 `max_tokens` 的多层来源：provider 默认、model 覆盖、CLI/env 运行时覆盖。
- 支持 thinking token 上限 `thinking_max_tokens`，用于控制 reasoning/thinking 阶段预算。
- Anthropic reasoning 请求透传 thinking budget，OpenAI reasoning 模型将 thinking token 上限映射为 effort 等级。
- 对不支持精确 thinking token 的 provider 保持兼容，按能力忽略或退化为 on/off。
- status/TUI 展示当前输出与 thinking 上限，方便确认实际生效配置。

**涉及文件**：
- `aemeath-core/src/config/models.rs`
- `aemeath-core/src/config/manager.rs`
- `aemeath-core/src/provider.rs`
- `aemeath-llm/src/provider.rs`
- `aemeath-llm/src/client.rs`
- `aemeath-llm/src/providers/anthropic.rs`
- `aemeath-llm/src/providers/openai_compatible/mod.rs`
- `aemeath-llm/src/providers/openai_compatible/non_stream.rs`
- `aemeath-cli/src/main.rs`
- `aemeath-cli/src/tui/status_bar.rs`

**关联**：
- Feature #11：复用 OpenAI `reasoning_effort` 管线。
- Feature #19 / #20：provider api 类型扩展需兼容 token 上限字段。
- Feature #22：已合并入本 feature 一并完成。
