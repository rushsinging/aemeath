#![deny(clippy::print_stdout, clippy::print_stderr)]

//! agent 下所有库的共享依赖层。

mod constants;

pub use crate::constants::COMPILED_VERSION;
pub(crate) use crate::constants::LOG_TARGET;

/// 运行时版本号：优先读 `AEMEATH_VERSION` 环境变量（方便本地测试覆盖），
/// fallback 到编译期注入的 [`COMPILED_VERSION`]。
///
/// 全仓库所有需要版本号的地方 MUST 引用此函数，NEVER 直接用 `CARGO_PKG_VERSION`。
/// 首次调用后用 `OnceLock` 缓存，保证整个进程返回同一个值。
pub fn version() -> &'static str {
    constants::CACHE.get_or_init(|| {
        std::env::var("AEMEATH_VERSION").unwrap_or_else(|_| COMPILED_VERSION.to_string())
    })
}

pub mod adapter;
pub mod config;
pub mod error;

#[cfg(test)]
#[path = "error_domain_tests.rs"]
mod error_domain_tests;
pub mod i18n;
pub mod ids;

#[cfg(test)]
#[path = "ids_tests.rs"]
mod ids_tests;
pub mod memory;
pub mod message;
pub mod reasoning;
pub mod session_types;
pub mod string_idx;
pub mod tools_vocab;

#[cfg(test)]
#[path = "tools_vocab_tests.rs"]
mod tools_vocab_tests;
