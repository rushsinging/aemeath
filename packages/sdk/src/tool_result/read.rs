use serde::{Deserialize, Serialize};

/// Typed result struct for `read` tool.
///
/// 字段由 Phase 0 任务 0.3/0.4 填充。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadResult {
    // Placeholder; will be filled in by Phase 0 任务 0.3 (core tools)
    // or 任务 0.4 (non-core tools).
    pub _placeholder: (),
}
