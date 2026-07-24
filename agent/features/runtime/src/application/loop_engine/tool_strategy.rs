//! Tool-strategy trait — minimal shared interface for per-adapter tool
//! orchestration flags.
//!
//! Main and Sub adapters have fundamentally different tool execution
//! pipelines (sink-based event routing vs progress-based reporting). This
//! trait does NOT abstract the full `execute_tools` method; it only covers
//! the `mark_tool_results_pending` behaviour that both adapters must expose
//! uniformly so the shared loop engine can request an
//! `InternalContinuation::ToolResults` drain.
//!
//! [`step_from_fuse_bypass`] is a free function (not on the trait) because
//! its logic is identical for both adapters and has no per-adapter state
//! dependency.
//!
//! The trait exists for interface documentation, not for dynamic dispatch.

use super::ToolStep;
use sdk::ids::ToolCallId;

pub(crate) trait ToolStrategy {
    /// Mark that tool results have been appended to messages so the next
    /// `drain_input` call returns `InternalContinuation::ToolResults` instead
    /// of `EmptyAndSealed`.
    fn mark_tool_results_pending(&mut self);
}

/// Convert a list of fuse-bypassed tool-call IDs into a [`ToolStep`].
///
/// - Empty list → [`ToolStep::Continue`]
/// - Non-empty list → [`ToolStep::ContinueWithFuseBypass`]
pub(crate) fn step_from_fuse_bypass(fuse_bypassed: Vec<ToolCallId>) -> ToolStep {
    if fuse_bypassed.is_empty() {
        ToolStep::Continue
    } else {
        ToolStep::ContinueWithFuseBypass(fuse_bypassed)
    }
}
