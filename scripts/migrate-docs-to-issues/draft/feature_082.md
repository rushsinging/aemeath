<!-- Migrated from: docs/feature/active.md#82 -->
### #82 Provider/Config 设计债收口

**状态**：未开始（2026-06-10 设计分析产出，待方案确认后实施）

**背景**：对 `agent/features/provider/**` 与 `agent/shared/src/config/models/**` 的设计分析确认整体分层方向正确：config 声明（`ModelsConfig` 的 source + driver 两级模型，source 自由命名、driver 封闭枚举）→ bootstrap 合并翻译（CLI/env/config 优先级合并、`ReasoningConfig` 决策树）→ `LlmClient` 门面（集中可观测性）→ driver 协议差异（`ChatApiDriver` strategy trait 吸收 OpenAI/Zhipu/LiteLLM/Volcengine 的请求字段差异：`max_tokens_field()` + `apply_reasoning_fields()`；流解析以宽容超集方式同时识别 `content` / `reasoning_content` 吸收响应差异）。问题集中在 `LlmClientPool::create_client` 旁路绕过 bootstrap 层自行实现解析，与主路径行为分叉。

**问题清单**：

1. **API key 解析双实现、优先级冲突**：主路径 `runtime/src/utils/bootstrap/provider_client.rs` 为 CLI → `AEMEATH_API_KEY` → driver 专属 env → `LLM_API_KEY` → config（env 优先于 config）；pool 路径 `provider/src/core/pool.rs::create_client` 为 config 优先于 env，不识别 `AEMEATH_API_KEY`，多出 `OPENAI_API_KEY` 兜底。driver→env 名映射表在两处重复定义。同一份配置下主 client 与子 agent client 可能使用不同 key，违反 DRY。
2. **pool 创建的 client 丢失 reasoning 配置**：`pool.rs::create_client` 硬编码 `reasoning = true`、`reasoning_config: None`，忽略 `model_entry.reasoning / reasoning_effort`，子 agent 不遵循模型级 reasoning 设置，与主路径决策树不一致。
3. **未知 driver 静默 fallback OpenAI**：`pool.rs` 与 `from_args.rs` 均为 `ProviderDriverKind::parse(...).unwrap_or(OpenAI)`，配置写错 driver 名不报错；与已归档 bug #85（Ollama 工厂未接线）同源。
4. **spec 漂移**：`specs/provider.md` 支持列表（OpenRouter/DeepSeek/Moonshot/DashScope/MiniMax 等）与 `ProviderDriverKind` 实际枚举（Anthropic/OpenAI/Zhipu/LiteLLM/Volcengine/Ollama）不一致——前者是"经 openai driver 可配置的厂商"，后者才是代码事实；`specs/config-compat.md` 所述 provider 默认值位置与实际（仅 Volcengine 有内置默认 source）不符。
5. **默认值与 legacy 字段散落**：`200000` max_tokens 兜底在 `client.rs` 与 `pool.rs` 各一份；user-agent 字符串在 `config/legacy.rs` 与 `openai_compatible/provider.rs` 各 format 一次；`max_retries: 10` 硬编码而 legacy `ApiConfig.timeout/retries` 未接入新路径。
6. **死代码**：`OpenAIProviderConfig::from_driver` 为 Anthropic driver 配置的 `/v1/messages` suffix 分支不可达（Anthropic 不走 OpenAI 兼容路径）。

**修复方向**：

1. 将 `resolve_api_key` / `reasoning_config` 决策下沉到 pool 与 bootstrap 共同可依赖的位置，`LlmClientPool::create_client` 复用同一套解析，消除两套优先级。
2. `ProviderDriverKind::parse` 失败时显式报错（附可用 driver 列表），移除 `unwrap_or(OpenAI)` 静默降级。
3. 散落默认值收口为单一常量来源；清理不可达 suffix 分支；决定 legacy `ApiConfig.timeout/retries` 接入或删除。
4. 同步修订 `specs/provider.md` / `specs/config-compat.md`（spec 修改前需用户同意）。

**验证**：
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 回归覆盖：unknown driver 报错路径、pool 与主路径 key 解析优先级一致性、bug #85 场景（ollama driver 路由）

**涉及路径**：
- `agent/features/provider/src/core/pool.rs`、`core/client.rs`、`contract.rs`
- `agent/features/runtime/src/utils/bootstrap/provider_client.rs`
- `agent/shared/src/config/models/types.rs`、`config/legacy.rs`
- `specs/provider.md`、`specs/config-compat.md`
