<!-- Migrated from: docs/feature/archived/019-zhipu-api-type.md -->
# #19 config model 支持 zhipu api 类型

**归档日期**：2026-05-04

**确认结果**：用户确认完成

**目标**：让 provider 配置的 `api` 字段支持显式 `"zhipu"`，把 GLM/Zhipu 模型路由到 Zhipu 专用请求/响应处理，而不是走通用 OpenAI Compatible 路径。

**实现**：
- provider api 类型增加 `zhipu`。
- 配置解析可识别 `api = "zhipu"`。
- LLM provider 构建与模型池路由支持 Zhipu 专用 provider。
- Zhipu 路径可独立处理 GLM 系列参数、thinking 字段与请求体限制。

**涉及文件**：
- `aemeath-core/src/config/models.rs`
- `aemeath-llm/src/provider.rs`
- `aemeath-llm/src/pool.rs`
- `aemeath-llm/src/client.rs`
- `aemeath-llm/src/providers/openai_compatible/mod.rs`
- `aemeath-llm/src/providers/openai_compatible/message_conversion.rs`

**关联**：
- Bug #13：Zhipu API 超大请求体返回空响应，后续仍需按 bug 跟踪请求体过大处理。
