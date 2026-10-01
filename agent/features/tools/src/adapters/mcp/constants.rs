//! MCP 子域常量（#1146 双轨归位）。

pub const DEFAULT_MAX_TOOL_RESPONSE_BYTES: usize = 1_048_576;

/// Default timeout for SSE endpoint handshake (seconds).
pub(crate) const SSE_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Default timeout for individual JSON-RPC requests via SSE stream (seconds).
///
/// Some SSE servers (e.g. z.ai) split large responses across multiple chunks
/// with long pauses between them. A shorter timeout with retries via stale
/// response acceptance is more reliable than a single long timeout.
pub(crate) const SSE_REQUEST_TIMEOUT_SECS: u64 = 15;

pub(crate) const BLOCKED_ENV_KEYS: &[&str] = &[
    "PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "HOME",
    "USER",
    "SHELL",
    "IFS",
    "CDPATH",
    "ENV",
    "BASH_ENV",
    "TERMINFO",
    "TERMINFO_DIRS",
    "LOCPATH",
    "NLSPATH",
];
