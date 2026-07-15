/// business/mod.rs — 业务规则（规则专家）：具体 provider 实现、流式解析与领域类型
/// - providers：anthropic / ollama / openai_compatible 具体实现
/// - stream：流式响应解析逻辑
/// - types：provider 领域 DTO（StreamResponse / Usage / StopReason 等)
pub(crate) mod error_log;
pub mod json_recovery;
pub mod providers;
pub mod stream;
pub mod types;

// ═══════════════════════════════════════════════════════════════════════
// Provider HTTP 超时常量 — 单一真相源
// 全 crate 的 timeout 值必须引用这里，NEVER 在 provider/stream 文件内重复定义。
// ═══════════════════════════════════════════════════════════════════════

/// 默认 HTTP 请求超时（秒），config 未设 `api.timeout` 时使用。
pub const DEFAULT_TIMEOUT_SECS: u64 = 1800; // 30 min

/// TCP 连接建立 + TLS 握手超时（秒），仅作用于连接阶段，不限制流式传输时长。
pub const CONNECT_TIMEOUT_SECS: u64 = 30;

/// Anthropic 流空闲超时（秒）：两次 SSE event 之间无数据的间隔超过则中止。
pub const ANTHROPIC_STREAM_IDLE_TIMEOUT_SECS: u64 = 90;

/// OpenAI compatible 流空闲超时（秒）：两次 SSE event 之间无数据的间隔超过则中止。
pub const OPENAI_STREAM_IDLE_TIMEOUT_SECS: u64 = 180;

/// Ollama 流空闲超时（秒）：两次数据块之间无数据的间隔超过则中止。
pub const OLLAMA_STREAM_IDLE_TIMEOUT_SECS: u64 = 180;

/// 流停滞警告阈值（秒）：超过则记录 warning 但不中止。
pub const STALL_THRESHOLD_SECS: u64 = 30;
