//! 系统提示文案：静态 system prompt 模板 + 日期标签。
//!
//! 迁自 runtime `prompt_build.rs` 的 `STATIC_SYSTEM_PROMPT_EN/ZH`。
//! 面向 LLM 注入的核心 system prompt 片段。

pub use super::constants::{STATIC_SYSTEM_PROMPT_EN, STATIC_SYSTEM_PROMPT_ZH};

/// 按语言选择静态系统提示模板（含 `{cwd_str}` / `{is_git}` 占位符）。未知 lang 回退英文。
pub fn static_system_prompt(lang: &str) -> &'static str {
    match lang {
        "zh" => STATIC_SYSTEM_PROMPT_ZH,
        _ => STATIC_SYSTEM_PROMPT_EN,
    }
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
