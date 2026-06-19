//! Typed result for the `memory` tool (non-core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;
use crate::memory::MemoryEntry;

/// Typed result returned by the `memory` tool.
///
/// `entries` re-uses the canonical `share::memory::MemoryEntry` type so the
/// memory tool's typed result is interoperable with the rest of the memory
/// subsystem without crossing DDD boundaries.
#[derive(Debug, Clone, Serialize, Deserialize, ToolSchema)]
pub struct MemoryResult {
    pub entries: Vec<MemoryEntry>,
}