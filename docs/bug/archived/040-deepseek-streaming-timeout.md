# Bug #40: DeepSeek 流式输出约 120 秒后 decode timeout

- **发现日期**：2026-05
- **归档日期**：2026-05-17
- **状态**：已确认修复
- **优先级**：高
- **修复提交**：5b925b6 `fix(llm): 移除流式请求总超时 refs #40`

## 症状

会话 `019e359e-4a50-77a7-a752-56f6ac115240` 在 DeepSeek `deepseek-v4-pro` 流式输出中多次失败，日志显示 `failed to read SSE line: error decoding response body`，外层记录为 `Streaming error` 后重试。

## 证据

同一 turn 多次在约 120 秒后失败；请求体约 180KB、100+ messages，并非超大请求体。reqwest 错误链底层为 operation timed out。

## 根因

OpenAI-compatible provider 构造 `reqwest::Client` 时设置了 total timeout=120s。该 timeout 覆盖整个 HTTP 请求生命周期，不适合长时间 SSE streaming；当模型正常长流输出超过 120 秒时，客户端主动截断响应，解析层表现为 decode/body 读取失败。

## 修复

默认 streaming HTTP client 不再设置 reqwest total timeout，继续依赖 `parse_openai_stream()` 内部的 `STREAM_IDLE_TIMEOUT` 处理无数据空闲超时，避免正常长流被固定总时长截断。

## 回归测试

`openai_streaming_http_client_has_no_total_timeout` 覆盖默认 streaming client builder 不包含 total timeout。

## 涉及路径

- `aemeath-llm/src/providers/openai_compatible/provider.rs`
- `aemeath-llm/src/providers/openai_compatible/tests.rs`
