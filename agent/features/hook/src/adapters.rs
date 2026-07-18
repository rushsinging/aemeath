//! Hook 技术适配器。
//!
//! #987 只迁移目录并保持行为；旧 HookRunner 兼容实现由 #926 退役。

pub(crate) mod legacy;
pub(crate) mod process;
