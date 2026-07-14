//! ReasoningPort — Workflow BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! 细化由 #919 负责。

use crate::business::agent_run::Run;
use provider::api::ReasoningLevel;

/// Workflow BC 的出站端口——决定本轮 reasoning effort。
///
/// Main Run 使用 `GraphDriven`（reasoning graph 决定 effort）。
/// Sub Run 使用 `EffortOnly`（无 graph，无设置时继承父）。
pub trait ReasoningPort: Send + Sync {
    /// 返回给定 Run 应使用的 reasoning level。
    fn effort(&self, run: &Run) -> ReasoningLevel;
}
