# Provider 实现

**Scope**：`agent/features/provider/**`——各 provider 的 HTTP / stream 调用实现。
**主触发**：改 `agent/features/provider/**`。
**次触发**：新增 provider。
**配套**：provider 的默认 base URL / 默认 model / API key 环境变量名等**配置**在 `agent/shared/src/config/`，见 `config-compat.md`；model guidance 文件见 `prompt.md`。

## 支持的 provider

Anthropic、OpenAI、OpenRouter、DeepSeek、Moonshot、Zhipu、DashScope、MiniMax、Ollama、OpenAICompatible。

实现层（`agent/features/provider/src/business/providers/`）按 driver 归并：

- **Anthropic 原生 driver**：`anthropic.rs`（+ `anthropic/message_conversion.rs`）——Messages API、流式/非流式、thinking budget、重试、tool use。
- **Ollama driver**：`ollama.rs`（+ `ollama/{non_stream,stream,conversion}.rs`）。
- **OpenAI 兼容 driver**：`openai_compatible.rs`（+ `openai_compatible/{non_stream,stream,message_conversion,provider,request_body,message_helpers,driver,reasoning}.rs`）——覆盖其余 OpenAI 兼容 provider。

provider 抽象与连接池：`agent/features/provider/src/core/{provider,client,pool}.rs`。

## 新增 provider

1. 在 `agent/features/provider/src/business/providers/` 添加实现（或复用 OpenAI 兼容 driver）。
2. 在 `agent/shared/src/config/`（见 `config-compat.md`）补默认 base URL / 默认 model / API key 环境变量名。
3. **SHOULD** 同步添加 model guidance 文件（见 `prompt.md`）。
4. **SHOULD** 成本追踪相关时同步更新 `pricing.rs`（见 `runtime.md`）。
