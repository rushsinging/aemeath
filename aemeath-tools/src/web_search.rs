use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "WebSearch" }
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
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let query = input["query"].as_str().unwrap_or("");
        let limit = input["limit"].as_u64().unwrap_or(5).min(10) as usize;
        
        if query.is_empty() {
            return ToolResult::error("Search query is required");
        }
        
        // 使用 DuckDuckGo 的 HTML 搜索页面作为简单的搜索源
        // 这是一个基础实现，不依赖外部 API key
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );
        
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
                    return ToolResult::error(format!("Search failed with status: {}", resp.status()));
                }
                
                match resp.text().await {
                    Ok(html_content) => {
                        // 解析简单的 HTML 结果
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

fn parse_duckduckgo_html(html: &str, limit: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // 简单的 HTML 解析 - DuckDuckGo HTML 结果在 class="result" 的 div 中
    // 查找 <a class="result__a" href="..."> 标题 </a>
    // 和 <a class="result__snippet"> 摘要 </a>

    let title_pattern = "<a class=\"result__a\"";
    let url_pattern = "href=\"";
    let snippet_pattern = "<a class=\"result__snippet\"";

    let mut pos = 0;
    while results.len() < limit {
        // 查找下一个结果块
        let result_start = match html[pos..].find("<div class=\"result") {
            Some(s) => s + pos,
            None => break,
        };

        // 查找结果块结束
        let result_end = match html[result_start..].find("</div>") {
            Some(e) => result_start + e.min(result_start + 5000),
            None => break,
        };

        let block = &html[result_start..result_end];

        // 提取标题和 URL
        let title_start = match block.find(title_pattern) {
            Some(s) => s,
            None => {
                pos = result_end;
                continue;
            }
        };

        // 找 href
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
        let url = &block[href_start..href_start + href_end];

        // DuckDuckGo URL 可能需要解码
        let url = url.replace("uddg=", "");
        let url = urlencoding::decode(&url).unwrap_or_default().to_string();

        // 找标题文本（在 > 和 </a> 之间）
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
        let title = block[title_text_start..title_text_start + title_text_end]
            .replace("&amp;", "&")
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
            .to_string();

        // 提取摘要
        let snippet = if let Some(snippet_pos) = block.find(snippet_pattern) {
            if let Some(text_pos) = block[snippet_pos..].find('>') {
                let start = snippet_pos + text_pos + 1;
                if let Some(end_pos) = block[start..].find("</a>") {
                    let raw_snippet = &block[start..start + end_pos];
                    // 应用相同的 HTML 实体解码
                    raw_snippet
                        .replace("&amp;", "&")
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
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        if !url.is_empty() && !title.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }

        pos = result_end;
    }

    results
}

// 简单的 URL 编码模块
mod urlencoding {
    pub fn encode(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                    c.to_string()
                } else {
                    format!("%{:02X}", c as u32)
                }
            })
            .collect()
    }
    
    pub fn decode(s: &str) -> Result<String, ()> {
        let mut result = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '%' {
                let hex = chars.next().and_then(|c1| chars.next().map(|c2| (c1, c2)));
                if let Some((c1, c2)) = hex {
                    let hex_str = format!("{}{}", c1, c2);
                    if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                        result.push(byte as char);
                    } else {
                        result.push('%');
                        result.push(c1);
                        result.push(c2);
                    }
                } else {
                    result.push('%');
                }
            } else {
                result.push(c);
            }
        }
        Ok(result)
    }
}