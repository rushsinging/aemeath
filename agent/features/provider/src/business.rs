/// business/mod.rs — 业务规则（规则专家）：具体 provider 实现、流式解析与领域类型
/// - providers：anthropic / ollama / openai_compatible 具体实现
/// - stream：流式响应解析逻辑
/// - types：provider 领域 DTO（StreamResponse / Usage / StopReason 等)
pub mod json_recovery;
pub mod providers;
pub mod stream;
pub mod types;

/// 默认 HTTP 请求超时（秒）。config 未设 api.timeout 时使用。
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;
