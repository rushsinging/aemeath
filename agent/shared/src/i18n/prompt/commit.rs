//! Commit 指南文案。
//!
//! 迁自 runtime `prompt_build.rs` 的 `build_commit_guidance` 模板。

/// Commit 指南模板（英文），含 `{trailer}` 占位符。
pub const COMMIT_GUIDANCE_EN: &str = r#"# Commit Message Guidance
When creating a git commit message:
- Before creating any git commit, invoke the built-in `commit` skill and follow its workflow.
- First inspect this repository's recent commit history and infer its Commit Style Context.- Prefer sampling commits that contain `Co-Authored-By`, for example: `git log --format=%B --grep='Co-Authored-By' -n 20`.
- If there are no useful co-author examples, sample recent ordinary commits with a small limit.
- Analyze title format, type/scope usage, body style, language, footer/trailer conventions, and whether AI co-author trailers are commonly used.
- Keep the final commit message consistent with this repository's existing style.
- Do not invent human co-authors.
- When an AI co-author trailer is appropriate, use exactly: `{trailer}`."#;

/// Commit 指南模板（中文），含 `{trailer}` 占位符。
pub const COMMIT_GUIDANCE_ZH: &str = r#"# Commit Message Guidance
创建 git commit message 时：
- 创建任何 git commit 前，调用内置的 `commit` skill 并遵循其工作流。
- 首先检查本仓库最近的提交历史，推断其 Commit Style Context。- 优先采样包含 `Co-Authored-By` 的提交，例如：`git log --format=%B --grep='Co-Authored-By' -n 20`。
- 如果没有有用的 co-author 示例，采样最近的普通提交（少量）。
- 分析标题格式、type/scope 用法、正文风格、语言、footer/trailer 约定，以及是否常用 AI co-author trailer。
- 保持最终 commit message 与本仓库的现有风格一致。
- 不要编造人类 co-author。
- 当 AI co-author trailer 适用时，精确使用：`{trailer}`。"#;

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
