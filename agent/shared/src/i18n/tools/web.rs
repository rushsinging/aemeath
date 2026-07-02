//! Web 工具文案（web_search/web_fetch 的 description）。

/// WebSearch description。
pub fn web_search(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"搜索网络以获取信息。返回带标题、URL 和摘要的搜索结果。

用法：
- 当需要查找当前信息、文档或问题答案时使用本工具
- 结果包含标题、URL 和简短摘要
- 可随后用 WebFetch 获取特定 URL 的完整内容"#
        }
        _ => {
            r#"Search the web for information. Returns search results with titles, URLs, and snippets.

Usage:
- Use this tool when you need to find current information, documentation, or answers to questions
- Results include titles, URLs, and brief snippets
- You can then use WebFetch to get full content from specific URLs"#
        }
    }
}

/// WebFetch description。
pub fn web_fetch(lang: &str) -> &'static str {
    match lang {
        "zh" => "通过 HTTP GET 获取 URL 内容。只读。对于 HTML 页面，会提取标题、将正文转换为 Markdown 并列出页面链接；大内容可能被截断。GitHub URL 优先用 `gh` CLI。",
        _ => "Fetches content from a URL via HTTP GET. Read-only. For HTML pages, extracts the title, converts the body to Markdown, and lists page links. Large content may be truncated. For GitHub URLs, prefer `gh` CLI.",
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
}
