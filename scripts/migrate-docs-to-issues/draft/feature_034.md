<!-- Migrated from: docs/feature/active.md#34 -->
### #34 Anthropic Claude 原生 Provider

**目标**：作为独立 LLM provider，支持直接调用 Anthropic Claude Messages API，与现有 OpenAI/OpenRouter/DeepSeek 等并列。

**已完成的改动**：

1. **ApiDriverKind::Anthropic**：`aemeath-core/src/provider.rs` 新增 `Anthropic` 变体，且为默认 provider。
2. **AnthropicProvider**（`aemeath-llm/src/providers/anthropic/`）：
    - 原生 Messages API 调用（`POST /v1/messages`）
    - 流式响应解析 + 非流式 fallback（streaming 失败且无 partial output 时自动降级）
    - Thinking budget（extended thinking）：`thinking_max_tokens > 0` 时自动开启 `thinking.type=enabled`
    - 指数退避重试（最多 10 次），支持 429 / 5xx 自动重试
    - 用户取消（CancellationToken）全路径支持
    - Prompt caching beta header（`anthropic-beta: prompt-caching-2024-07-31`）
3. **消息转换**（`anthropic/message_conversion.rs`）：TrackingHandler 检测 partial output 避免非流式 fallback 重复输出。
4. **请求构造**（`types.rs::CreateMessageRequest`）：thinking budget 序列化、tool schema 嵌入。
5. **池化集成**（`pool.rs`）：Anthropic driver 走独立 provider 分支，不经过 OpenAICompatible wrapper。

**涉及路径**：
- `aemeath-core/src/provider.rs`（ApiDriverKind::Anthropic）
- `aemeath-llm/src/providers/anthropic/mod.rs`（AnthropicProvider 实现）
- `aemeath-llm/src/providers/anthropic/message_conversion.rs`（非流式 fallback + TrackingHandler）
- `aemeath-llm/src/client.rs`（with_provider 匹配 Anthropic 分支）
- `aemeath-llm/src/pool.rs`（Anthropic 跳过 OpenAIProviderConfig 构建）
- `aemeath-llm/src/types.rs`（CreateMessageRequest thinking 序列化）

**测试**：`anthropic_request_serializes_thinking_budget`、`anthropic_request_omits_thinking_when_budget_zero` + `provider.rs` 6 个单元测试。

---
