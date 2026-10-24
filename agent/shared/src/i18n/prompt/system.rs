//! 系统提示文案：静态 system prompt 模板 + 日期标签。
//!
//! 迁自 runtime `prompt_build.rs` 的 `STATIC_SYSTEM_PROMPT_EN/ZH`。
//! 面向 LLM 注入的核心 system prompt 片段。

pub use super::constants::{
    STATIC_SYSTEM_PROMPT_EN, STATIC_SYSTEM_PROMPT_ZH, TOOL_TIMEOUT_GUIDANCE_DISABLED_EN,
    TOOL_TIMEOUT_GUIDANCE_DISABLED_ZH, TOOL_TIMEOUT_GUIDANCE_ENABLED_EN,
    TOOL_TIMEOUT_GUIDANCE_ENABLED_ZH,
};

/// 按语言选择静态系统提示模板（含 `{cwd_str}` / `{is_git}` 占位符）。未知 lang 回退英文。
pub fn static_system_prompt(lang: &str) -> &'static str {
    match lang {
        "zh" => STATIC_SYSTEM_PROMPT_ZH,
        _ => STATIC_SYSTEM_PROMPT_EN,
    }
}

/// 按会话后台化开关与语言选择 `timeout` 预算 guidance。
///
/// 开启档声明 24h 默认预算并劝退无谓设置；关闭档声明 30m 默认并要求
/// 长任务显式传 `timeout`。未知 lang 回退英文（与静态模板一致）。
pub fn tool_timeout_guidance(lang: &str, backgrounding_enabled: bool) -> &'static str {
    match (backgrounding_enabled, lang) {
        (true, "zh") => TOOL_TIMEOUT_GUIDANCE_ENABLED_ZH,
        (true, _) => TOOL_TIMEOUT_GUIDANCE_ENABLED_EN,
        (false, "zh") => TOOL_TIMEOUT_GUIDANCE_DISABLED_ZH,
        (false, _) => TOOL_TIMEOUT_GUIDANCE_DISABLED_EN,
    }
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
