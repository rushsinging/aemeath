pub mod client;
pub mod config;
pub mod response_limit;
pub mod validation;

pub use client::McpClient;
pub use config::{McpServerConfig, McpToolDef, McpTransportKind};
pub use response_limit::{limit_tool_response, DEFAULT_MAX_TOOL_RESPONSE_BYTES};
pub use validation::{redact_headers, validate_remote_url};

#[cfg(test)]
mod tests;
