//! Workflow Graph 配置（#2085）：MVP 总开关，完全交付前默认关闭。

use serde::{Deserialize, Serialize};

/// Workflow 配置段。
///
/// `enabled = false`（默认）时 composition 不注入 Workflow backend，六个
/// `workflow_*` Tool 不进入 catalog；开启后注入内存 backend。Workflow
/// MVP 验收（#2091）通过前保持默认关闭。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// 六个 `workflow_*` Tool 的总开关；默认 `false`。
    #[serde(default)]
    pub enabled: bool,
}
