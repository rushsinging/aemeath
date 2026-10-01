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
#[path = "web_tests.rs"]
mod tests;
