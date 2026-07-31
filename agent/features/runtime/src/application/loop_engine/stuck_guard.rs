use crate::application::loop_engine::chat::stall::StallDetector;
use crate::application::tool::agent::ToolCall;
use crate::application::tool::coordination::loop_guard::{ToolCallFuse, ToolFuseDecision};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StuckDecision {
    Allow,
    SoftBlock { reason: String },
    HardPause { reason: String },
}

/// Guard against stuck loops (repeated text, tool call loops, timeout).
///
/// #1248 Task 6: Stop hook block counting has been moved to `Run` domain.
/// `record_stop_hook_block` and `stop_hook_block_limit`/`stop_hook_block_count`
/// are removed — the shared Loop now uses `Run::record_stop_hook_block()`.
pub struct StuckGuard {
    stall: StallDetector,
    tool_fuse: ToolCallFuse,
    text_stall_count: usize,
}

impl StuckGuard {
    pub fn new() -> Self {
        Self {
            stall: StallDetector::new(),
            tool_fuse: ToolCallFuse::new(),
            text_stall_count: 0,
        }
    }

    pub fn inspect_text(&mut self, text: &str) -> StuckDecision {
        if self.stall.record_text(text) {
            self.text_stall_count = self.text_stall_count.saturating_add(1);
            let reason = format!(
                "assistant text repeated three times (stuck count {})",
                self.text_stall_count
            );
            if self.text_stall_count >= 3 {
                StuckDecision::HardPause { reason }
            } else {
                StuckDecision::SoftBlock { reason }
            }
        } else {
            StuckDecision::Allow
        }
    }

    pub fn inspect_tool(&mut self, call: &ToolCall) -> StuckDecision {
        match self.tool_fuse.inspect(call) {
            ToolFuseDecision::Allow => StuckDecision::Allow,
            ToolFuseDecision::SoftBlock { reason } => StuckDecision::SoftBlock { reason },
            ToolFuseDecision::HardPause { reason } => StuckDecision::HardPause { reason },
        }
    }
}
