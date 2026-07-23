# Provider 实现

**Scope**：`agent/features/provider/**`——各 provider 的 HTTP / stream 调用实现。
**主触发**：改 `agent/features/provider/**`。
**次触发**：新增 provider。
**配套**：provider 的默认 base URL / 默认 model / API key 环境变量名等**配置**在 `agent/shared/src/config/`，见 `config-compat.md`；model guidance 文件见 `prompt.md`。

## 3.8.1. 支持的 provider

Anthropic、OpenAI、OpenRouter、DeepSeek、Moonshot、Zhipu、DashScope、MiniMax、Ollama、OpenAICompatible。

实现按六边形职责组织：

- `domain/capability.rs`：driver 与模型能力解析。
- `domain/invoke.rs`：既有调用 DTO 与统一流结果语义。
- `ports.rs`：Provider、stream handler 等端口。
- `adapters/client.rs`、`adapters/pool.rs`、`adapters/transport.rs`：客户端、连接池和兼容 wiring。
- `adapters/anthropic.rs`（+ `adapters/anthropic/`）：Messages API、流式/非流式、thinking budget、重试、tool use。
- `adapters/ollama.rs`（+ `adapters/ollama/`）。
- `adapters/openai_compatible.rs`（+ `adapters/openai_compatible/`）：覆盖其余 OpenAI 兼容 provider。
- `lib.rs`：跨 crate 使用的窄 façade；消费方 **MUST** 从 crate root 导入，**NEVER** 穿透 `domain` / `ports` / `adapters`。

## 3.8.2. 新增 provider

1. 在 `agent/features/provider/src/adapters/` 添加实现（或复用 OpenAI 兼容 driver）。
2. 在 `agent/shared/src/config/`（见 `config-compat.md`）补默认 base URL / 默认 model / API key 环境变量名。
3. **SHOULD** 同步添加 model guidance 文件（见 `prompt.md`）。
4. **SHOULD** 成本追踪相关时同步更新 `pricing.rs`（见 `runtime.md`）。
