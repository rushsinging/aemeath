//! 项目级 LLM 文案 i18n catalog（单一真相）。
//!
//! 三层（prompt / runtime / tools）共同依赖 `share`，统一从此模块取面向 LLM 的文案。
//! 详见 `docs/superpowers/specs/2026-06-20-project-i18n-path-context-design.md`。
//!
//! ## API 形态
//!
//! 强类型函数式，不用字符串 key：
//! - 无参文案返回 `&'static str`
//! - 带参文案返回 `String`，内部用 `{placeholder}` + `replace`
//!
//! 默认分支（`_`）返回英文，`"zh"` 分支返回中文。

/// 默认语言代码。所有 `match lang` 的默认分支（`_`）对应此语言。
pub const DEFAULT_LANG: &str = "en";

/// 语言代码类型别名。全仓库面向 LLM 的文案统一用此类型传递 lang。
///
/// 取值：`"en"`（默认）/ `"zh"`。未知值按 [`DEFAULT_LANG`] 兜底。
pub type Lang = str;

pub mod prompt;
pub mod runtime;
pub mod tools;
