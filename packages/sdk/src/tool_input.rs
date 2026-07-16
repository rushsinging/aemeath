//! Typed tool input structs for the built-in tools.
//!
//! Thin re-export layer mirroring [`crate::tool_result`]. The authoritative
//! definitions live in `share::tool::types::*`; this module lets `apps/cli`
//! (TUI) deserialize tool-call arguments without depending on the business
//! layer.
//!
//! All structs derive `Deserialize + Default`, so callers can safely do:
//! ```ignore
//! let args: sdk::tool_input::TaskUpdateInput =
//!     serde_json::from_value(input.clone()).unwrap_or_default();
//! ```
//! serde `#[serde(alias = "...)]` attributes are honoured, eliminating the
//! snake_case / camelCase mismatch that `str_arg` caused (issue #839).
pub use share::tool::types::agent::AgentInput;
pub use share::tool::types::bash::BashInput;
pub use share::tool::types::edit::EditInput;
pub use share::tool::types::enter_worktree::EnterWorktreeInput;
pub use share::tool::types::exit_worktree::ExitWorktreeInput;
pub use share::tool::types::glob::GlobInput;
pub use share::tool::types::grep::GrepInput;
pub use share::tool::types::lsp::LspInput;
pub use share::tool::types::plan_mode::{EnterPlanModeInput, ExitPlanModeInput};
pub use share::tool::types::read::ReadInput;
pub use share::tool::types::skill::SkillInput;
pub use share::tool::types::task_create::TaskCreateInput;
pub use share::tool::types::task_get::TaskGetInput;
pub use share::tool::types::task_list_create::TaskListCreateInput;
pub use share::tool::types::task_stop::TaskStopInput;
pub use share::tool::types::task_update::TaskUpdateInput;
pub use share::tool::types::web_fetch::WebFetchInput;
pub use share::tool::types::web_search::WebSearchInput;
pub use share::tool::types::write::WriteInput;
