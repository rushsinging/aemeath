//! Tool-step construction shared by Main and Sub tool orchestration.

use super::ToolStep;
use sdk::ids::ToolCallId;

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
