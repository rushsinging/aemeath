pub mod client;
pub mod config;
mod constants;
pub mod response_limit;
pub mod sse;
mod sse_stream;
pub mod validation;

pub use client::McpClient;
pub use config::{McpServerConfig, McpToolDef, McpTransportKind};
pub use constants::DEFAULT_MAX_TOOL_RESPONSE_BYTES;
pub use response_limit::limit_tool_response;
pub use validation::{redact_headers, validate_remote_url};

#[cfg(test)]
mod tests;
