//! Typed tool input structs for the built-in tools.
//!
//! Thin re-export layer mirroring [`crate::tool_result`]. The authoritative
//! definitions live in `tools::types::*`; this module lets `apps/cli`
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
pub use tools::types::agent::AgentInput;
pub use tools::types::bash::BashInput;
pub use tools::types::edit::EditInput;
pub use tools::types::enter_worktree::EnterWorktreeInput;
pub use tools::types::exit_worktree::ExitWorktreeInput;
pub use tools::types::glob::GlobInput;
pub use tools::types::grep::GrepInput;
pub use tools::types::lsp::LspInput;
pub use tools::types::plan_mode::{EnterPlanModeInput, ExitPlanModeInput};
pub use tools::types::read::ReadInput;
pub use tools::types::task_create::TaskCreateInput;
pub use tools::types::task_get::TaskGetInput;
pub use tools::types::task_list_create::TaskListCreateInput;
pub use tools::types::task_stop::TaskStopInput;
pub use tools::types::task_update::TaskUpdateInput;
pub use tools::types::web_fetch::WebFetchInput;
pub use tools::types::web_search::WebSearchInput;
pub use tools::types::write::WriteInput;
