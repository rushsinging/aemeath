//! i18n 层共享常量（#1146 归位：自 `i18n.rs` 抽出，经 re-export 保持
//! `share::i18n::DEFAULT_LANG` 路径不变）。

/// 默认语言代码。所有 `match lang` 的默认分支（`_`）对应此语言。
pub const DEFAULT_LANG: &str = "en";
