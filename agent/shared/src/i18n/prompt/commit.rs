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
mod tests {
    use super::*;

    #[test]
    fn commit_guidance_bilingual_and_fallback_en() {
        let zh = commit_guidance_template("zh");
        let en = commit_guidance_template("en");
        assert!(zh.contains("创建"));
        assert!(en.contains("creating"));
        assert_eq!(commit_guidance_template("fr"), en);
    }

    #[test]
    fn commit_guidance_contains_trailer_placeholder() {
        for s in [
            commit_guidance_template("zh"),
            commit_guidance_template("en"),
        ] {
            assert!(s.contains("{trailer}"));
            assert!(s.contains("Co-Authored-By"));
        }
    }
}
