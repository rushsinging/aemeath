//! Web 工具文案（web_search/web_fetch 的 description）。

/// WebSearch description。
pub fn web_search(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"搜索网络以获取当前信息、文档或问题答案。返回标题、URL 和摘要；随后用 WebFetch 获取特定 URL 的完整内容。"#
        }
        _ => {
            r#"Search the web for current information, documentation, or answers to questions. Returns titles, URLs, and snippets; follow up with WebFetch to read a full page."#
        }
    }
}

/// WebFetch description。
pub fn web_fetch(lang: &str) -> &'static str {
    match lang {
        "zh" => "通过 HTTP GET 获取 URL 内容。只读。HTML 页面转为 Markdown，大内容可能被截断。GitHub URL 优先用 `gh` CLI。",
        _ => "Fetches content from a URL via HTTP GET. Read-only. HTML pages are converted to Markdown; large content may be truncated. For GitHub URLs, prefer `gh` CLI.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_bilingual_and_fallback() {
        assert!(web_search("zh").contains("搜索网络"));
        assert!(web_search("en").contains("Search the web"));
        assert_eq!(web_search("fr"), web_search("en"));
        assert!(web_fetch("zh").contains("获取 URL 内容"));
        assert!(web_fetch("en").contains("Fetches content"));
    }

    /// 收敛后的 description 长度预算：en/zh 均不得超过 200 字符。
    #[test]
    fn trimmed_web_descriptions_fit_the_200_char_budget() {
        for (zh, en) in [
            (web_search("zh"), web_search("en")),
            (web_fetch("zh"), web_fetch("en")),
        ] {
            assert!(
                zh.chars().count() <= 200,
                "zh too long ({}): {zh}",
                zh.chars().count()
            );
            assert!(
                en.chars().count() <= 200,
                "en too long ({}): {en}",
                en.chars().count()
            );
        }
    }
}
