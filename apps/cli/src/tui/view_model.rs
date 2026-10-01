mod constants;
pub mod conversation;
pub mod dialog;
pub mod display_text;
pub mod input;
pub mod live_status;
pub mod markdown_spacing;
pub mod nesting;
pub mod output;
mod state;
pub mod status;
pub mod style;
pub mod tool_name;

pub use dialog::{DialogActionViewModel, DialogKind, DialogViewModel};
pub use input::InputAreaViewModel;
pub use live_status::{LiveStatusViewModel, SpinnerLineView};
pub use nesting::{allowed_child, MAX_BLOCK_DEPTH};
pub use output::{
    AgentMetaView, AskUserBatchBlockView, AskUserPhaseView, AskUserSlotView, BlockNode,
    OutputBlockKind, OutputRenderWindow, OutputViewModel, TextBlockView, ToolCallBlockView,
    ToolResultBlockView, ToolSemanticStatus,
};
pub use status::{
    StatusContextViewModel, StatusLineViewModel, StatusNoticeViewKind, StatusNoticeViewModel,
    StatusRuntimeViewModel, StatusSegment, StatusSeverity, StatusViewModel, StatusWorktreeKind,
};
pub use style::SemanticStyle;

#[cfg(test)]
#[path = "view_model_tests.rs"]
mod tests;
