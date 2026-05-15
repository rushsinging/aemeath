use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::Value;

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }
    fn description(&self) -> &str {
        "Search the web for information. Returns search results with titles, URLs, and snippets.\n\nUsage:\n- Use this tool when you need to find current information, documentation, or answers to questions\n- Results include titles, URLs, and brief snippets\n- You can then use WebFetch to get full content from specific URLs"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default 5, max 10)",
                    "minimum": 1,
                    "maximum": 10
                }
            },
            "required": ["query"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let query = input["query"].as_str().unwrap_or("");
        let limit = input["limit"].as_u64().unwrap_or(5).min(10) as usize;

        if query.is_empty() {
            return ToolResult::error("Search query is required");
        }

        let encoded_query = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
        let url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (compatible; AemeathCLI/1.0)")
            .build()
        {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to create HTTP client: {}", e)),
        };

        match client.get(&url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    return ToolResult::error(format!(
                        "Search failed with status: {}",
                        resp.status()
                    ));
                }

                match resp.text().await {
                    Ok(html_content) => {
                        let results = parse_duckduckgo_html(&html_content, limit);

                        if results.is_empty() {
                            return ToolResult::success("No search results found");
                        }

                        let output = results
                            .iter()
                            .enumerate()
                            .map(|(i, r)| {
                                format!(
                                    "{}. {}\n   URL: {}\n   {}\n",
                                    i + 1,
                                    r.title,
                                    r.url,
                                    r.snippet
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        ToolResult::success(output)
                    }
                    Err(e) => ToolResult::error(format!("Failed to read response: {}", e)),
                }
            }
            Err(e) => ToolResult::error(format!("Search request failed: {}", e)),
        }
    }
}

struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .replace("&copy;", "©")
        .replace("&reg;", "®")
        .replace("&trade;", "™")
        .trim()
        .to_string()
}

fn parse_duckduckgo_html(html: &str, limit: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    let title_pattern = "class=\"result__a\"";
    let url_pattern = "href=\"";
    let snippet_pattern = "class=\"result__snippet\"";

    let mut pos = 0;
    while results.len() < limit {
        let result_start = match html[pos..].find("<div class=\"result ") {
            Some(s) => s + pos,
            None => break,
        };

        let result_end = match html[result_start..].find("</div>") {
            Some(e) => result_start + e.min(result_start + 5000),
            None => break,
        };

        let block = &html[result_start..result_end];

        let title_start = match block.find(title_pattern) {
            Some(s) => s,
            None => {
                pos = result_end;
                continue;
            }
        };

        let href_start_in_block = match block[title_start..].find(url_pattern) {
            Some(s) => s + url_pattern.len(),
            None => {
                pos = result_end;
                continue;
            }
        };
        let href_start = title_start + href_start_in_block;

        let href_end = match block[href_start..].find('"') {
            Some(e) => e,
            None => {
                pos = result_end;
                continue;
            }
        };
        let raw_url = &block[href_start..href_start + href_end];

        // DuckDuckGo wraps URLs: extract the actual URL from uddg= parameter
        let decoded_url = percent_decode_str(raw_url).decode_utf8_lossy().to_string();
        let actual_url = if let Some(idx) = decoded_url.find("uddg=") {
            &decoded_url[idx + 5..]
        } else {
            &decoded_url
        };

        let title_text_start = match block[href_start + href_end..].find('>') {
            Some(s) => href_start + href_end + s + 1,
            None => {
                pos = result_end;
                continue;
            }
        };

        let title_text_end = match block[title_text_start..].find("</a>") {
            Some(e) => e,
            None => {
                pos = result_end;
                continue;
            }
        };
        let title =
            decode_html_entities(&block[title_text_start..title_text_start + title_text_end]);

        let snippet = if let Some(snippet_pos) = block.find(snippet_pattern) {
            if let Some(text_pos) = block[snippet_pos..].find('>') {
                let start = snippet_pos + text_pos + 1;
                if let Some(end_pos) = block[start..].find("</a>") {
                    decode_html_entities(&block[start..start + end_pos])
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        if !actual_url.is_empty() && !title.is_empty() {
            results.push(SearchResult {
                title,
                url: actual_url.to_string(),
                snippet,
            });
        }

        pos = result_end;
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duckduckgo_html_handles_anchor_rel_before_class() {
        let html = r#"
            <div class="result results_links results_links_deep web-result ">
              <div class="links_main links_deep result__body">
                <h2 class="result__title">
                  <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F&amp;rut=abc">Rust Programming Language</a>
                </h2>
                <a class="result__snippet" href="/">A language empowering everyone.</a>
              </div>
            </div>
        "#;

        let results = parse_duckduckgo_html(html, 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/&amp;rut=abc");
        assert_eq!(results[0].snippet, "A language empowering everyone.");
    }
}
