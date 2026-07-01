//! 过渡层：SpinnerPhase / HookOutcome 从 conversation 模块 re-export。
//! SpinnerModel 保留旧版本（含 active 字段），直到 RuntimeModel 删除。

pub use crate::tui::model::conversation::spinner::{HookOutcome, SpinnerPhase};

/// 旧版 SpinnerModel，保留 `active` 字段供 RuntimeModel 过渡使用。
/// Phase 3 删除 RuntimeModel 后，此类型一并删除，统一用 conversation::spinner::SpinnerModel。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpinnerModel {
    pub active: bool,
    pub phase: Option<SpinnerPhase>,
}
