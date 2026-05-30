//! 补全候选副作用。
//!
//! 纯解析和补全状态归 `tui::model::input::completion`；需要访问文件系统
//! 的候选生成归 Effect 层，避免 Input Model 直接执行 IO。

pub mod files;

pub use files::generate_file_suggestions;
