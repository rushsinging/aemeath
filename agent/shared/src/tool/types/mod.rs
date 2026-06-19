//! Typed result structs for every tool, plus the small support types they
//! reference. Defined here in `share` so all feature crates
//! (`tools`, `runtime`, `compact`, …) can refer to them without depending on
//! `sdk` (which is a thin re-export / protocol facade — see
//! `docs/design/outline.md` §依赖铁律 and
//! `docs/snapshot/specs/047-ddd-redesign.md` §6.4.7 for the boundary).
//!
//! The same structs are `pub use`-re-exported by `packages/sdk::tool_result`
//! for the `cli` consumer and any future `server` consumer.
//!
//! See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
//! (plan 方案 D) for the design rationale and the per-tool field tables.

// ---------------------------------------------------------------------------
// Support types referenced by more than one tool's result struct.
// Kept in `share` (not in any feature) so the result structs can stay
// `use`-able from any feature without inverting DDD boundaries.
// ---------------------------------------------------------------------------

pub mod support;

pub mod read;
pub mod write;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod web_fetch;
pub mod bash;
pub mod agent;
pub mod enter_worktree;
pub mod exit_worktree;
pub mod ask_user;

// Re-exports for ergonomic `use share::tool::types::XxxResult;`.
pub use agent::AgentResult;
pub use ask_user::AskUserQuestionResult;
pub use bash::BashResult;
pub use edit::EditResult;
pub use enter_worktree::EnterWorktreeResult;
pub use exit_worktree::ExitWorktreeResult;
pub use glob::GlobResult;
pub use grep::GrepResult;
pub use read::ReadResult;
pub use web_fetch::WebFetchResult;
pub use write::WriteResult;