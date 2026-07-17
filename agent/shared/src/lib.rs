#![deny(clippy::print_stdout, clippy::print_stderr)]

//! agent 下所有库的共享依赖层。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:shared";

/// 编译期注入的版本号，来源于 build.rs 从 git tag 注入的 `AEMEATH_VERSION`；
/// 取不到时 fallback 到 `Cargo.toml` 的 `version`（占位符 `0.0.0`）。
pub const COMPILED_VERSION: &str = match option_env!("AEMEATH_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// 运行时版本号：优先读 `AEMEATH_VERSION` 环境变量（方便本地测试覆盖），
/// fallback 到编译期注入的 [`COMPILED_VERSION`]。
///
/// 全仓库所有需要版本号的地方 MUST 引用此函数，NEVER 直接用 `CARGO_PKG_VERSION`。
/// 首次调用后用 `OnceLock` 缓存，保证整个进程返回同一个值。
pub fn version() -> &'static str {
    static CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        std::env::var("AEMEATH_VERSION").unwrap_or_else(|_| COMPILED_VERSION.to_string())
    })
}

pub mod adapter;
pub mod config;
pub mod error;
pub mod i18n;
pub mod memory;
pub mod message;
pub mod reasoning;
pub mod session_types;
pub mod skill_ops;
pub mod string_idx;
pub mod task;
pub mod tool;
