# #20 config model 支持 litellm api 类型

**归档日期**：2026-05-04

**确认结果**：用户确认完成

**目标**：让 provider 配置的 `api` 字段支持显式 `"litellm"`，对接 LiteLLM Proxy 的统一 OpenAI 兼容接口，并透传 LiteLLM 扩展字段。

**实现**：
- provider api 类型增加 `litellm`。
- 配置解析可识别 `api = "litellm"`。
- LLM provider 构建与模型池路由支持 LiteLLM 路径。
- 支持 `model = "<provider>/<model>"` 形式透传给 LiteLLM Proxy。
- 兼容 OpenAI Compatible 请求/响应结构，并保留 thinking/reasoning 等扩展字段透传空间。

**涉及文件**：
- `aemeath-core/src/config/models.rs`
- `aemeath-llm/src/provider.rs`
- `aemeath-llm/src/pool.rs`
- `aemeath-llm/src/client.rs`
- `aemeath-llm/src/providers/openai_compatible/mod.rs`
- `aemeath-llm/src/providers/openai_compatible/non_stream.rs`
- `aemeath-llm/src/providers/openai_compatible/message_conversion.rs`

**关联**：
- Feature #11：OpenAI reasoning effort 字段可通过 LiteLLM 路径透传。
- Feature #15：输出/thinking token 上限需兼容 LiteLLM provider 配置。
