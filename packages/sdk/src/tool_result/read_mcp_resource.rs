use serde::{Deserialize, Serialize};

/// Typed result struct for `read_mcp_resource` tool.
///
/// 字段由 Phase 0 任务 0.3/0.4 填充。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadMcpResourceResult {
    // Placeholder; will be filled in by Phase 0 任务 0.3 (core tools)
    // or 任务 0.4 (non-core tools).
    pub _placeholder: (),
}
