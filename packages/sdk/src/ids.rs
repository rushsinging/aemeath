//! 转发模块：id 类型已下沉 `share::ids`（runtime / context / policy / audit
//! 的 domain 层依赖 share 而非 sdk 外圈）。保留本模块与 crate 根 re-export，
//! 使 `sdk::ids::*` 与 `sdk::*` 旧路径零改动。
pub use share::ids::*;
