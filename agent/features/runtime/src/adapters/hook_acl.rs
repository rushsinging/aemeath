//! Hook BC → Runtime 纯值投影。
//!
//! #1381 迁移：类型定义和 project 函数暂在 application::hook_types。
//! 后续 HookPort trait 返回 RuntimeHookDispatch 后，本文件承载 BC 映射。

pub use crate::application::hook_types::*;
