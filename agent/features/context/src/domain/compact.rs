//! Compact 家族子模块（五级管线）。
//!
//! 设计文档：`docs/design/02-modules/context-management/02-compact.md`

mod autocompact;
mod microcompact;
mod restore;

pub use crate::domain::token_budget::*;
pub use autocompact::*;
pub use microcompact::{microcompact_chain, microcompact_messages};
pub use restore::*;

/// Compact 进度阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactStage {
    Preparing,
    Summarizing,
    Finalizing,
}

impl CompactStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Summarizing => "summarizing",
            Self::Finalizing => "finalizing",
        }
    }
}
