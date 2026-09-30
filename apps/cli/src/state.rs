//! 状态容器（#1146 placement 归位）。

use std::sync::atomic::{AtomicBool, AtomicUsize};

/// TUI 是否持有终端（raw mode + alternate screen）。为真时向 stderr 写 panic
/// 会糊到屏幕上，故此时只落 panic.log，不打印 stderr。
/// TUI 是否持有终端（raw mode + alternate screen）。为真时向 stderr 写 panic
/// 会糊到屏幕上，故此时只落 panic.log，不打印 stderr。
pub(crate) static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) static CURRENT_RUN: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub(crate) static SESSION_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
