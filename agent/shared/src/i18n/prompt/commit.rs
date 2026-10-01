//! Commit 指南文案。
//!
//! 迁自 runtime `prompt_build.rs` 的 `build_commit_guidance` 模板。

pub use super::constants::{COMMIT_GUIDANCE_EN, COMMIT_GUIDANCE_ZH};

/// 按语言选择 commit 指南模板（含 `{trailer}` 占位符）。未知 lang 回退英文。
pub fn commit_guidance_template(lang: &str) -> &'static str {
    match lang {
        "zh" => COMMIT_GUIDANCE_ZH,
        _ => COMMIT_GUIDANCE_EN,
    }
}

#[cfg(test)]
#[path = "commit_tests.rs"]
mod tests;
