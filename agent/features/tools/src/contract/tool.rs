use super::ToolExecutionContext;
use async_trait::async_trait;
use serde_json::Value;
use share::tool::{PathAccess, ToolResult};

#[async_trait]
pub trait Tool: Send + Sync {

    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    /// Timeout for this tool in seconds (default 120s, override for long-running tools)
    fn timeout_secs(&self) -> u64 {
        120
    }

    /// Declare path fields in the input JSON that the PolicyEngine should
    /// validate and normalise before `call` is invoked.
    ///
    /// Override in tools that access files or search directories.
    fn path_accesses(&self) -> &'static [PathAccess] {
        &[]
    }

    /// Whether the given input is safe for auto-approval.
    ///
    /// Override in tools like Bash that need to inspect the command string.
    fn is_input_safe(&self, _input: &Value) -> bool {
        false
    }

    /// Whether the PolicyEngine should enforce "read-before-write" for this tool.
    ///
    /// When overridden to `true`, the engine will deny the call (after path
    /// normalisation) if any `PathKind::File` access resolves to an **existing**
    /// file that has not been recorded in the session's `read_files` set.
    ///
    /// Override in tools like Edit / Write that modify existing files.
    fn requires_read_before_write(&self) -> bool {
        false
    }

    /// Execute the tool.
    ///
    /// The return type is `Self::Result`, which defaults to
    /// `ToolResult<serde_json::Value>` for backward compatibility. Tools
    /// that opt into a typed payload override the `Result` associated
    /// type and return `ToolResult<MyResult>` directly.
    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult;
}
