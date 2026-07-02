# 增强 WebFetch HTML→Markdown 清洗 + title/links 提取

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在保持 `WebFetch` 轻量、不引入浏览器的前提下，为其增加 HTML 解析、Markdown 清洗、标题提取与链接提取能力。

**Architecture:** 继续使用 `curl` 获取原始响应；获取成功后按内容类型分流——非 HTML 保持原样返回，HTML 则通过 `scraper` 解析结构、`htmd` 转换为 Markdown，并提取 `title` 与去重后的 `links`。新增逻辑封装为独立模块，便于测试和后续扩展。

**Tech Stack:** `scraper = "0.27"`（HTML 解析与 CSS 选择）、`htmd = "0.5"`（HTML→Markdown 转换）。

---

## 文件结构映射

| 文件 | 责任 |
|---|---|
| `agent/features/tools/Cargo.toml` | 新增 `scraper`、`htmd` 依赖 |
| `agent/features/tools/src/business/web_fetch.rs` | 调整 `call` 流程：获取 body 后按内容类型分流；调用解析模块；填充 `WebFetchResult` |
| `agent/features/tools/src/business/web_fetch/extract.rs`（新建） | 核心提取逻辑：HTML 解析、标题提取、内容区域选择、Markdown 转换、链接收集与绝对化 |
| `agent/features/tools/src/business/web_fetch/tests.rs`（新建） | 单元测试覆盖 HTML 清洗、title/links 提取、大小限制、非 HTML fallback |
| `agent/shared/src/tool/types/web_fetch.rs` | `WebFetchResult` 新增 `links: Vec<String>` 字段；`WebFetchInput` 可选新增 `max_links`/`extract_content`（本期不加，保持简单） |
| `packages/sdk/src/tool_result/web_fetch.rs` | 复用 `share::tool::types::web_fetch::WebFetchResult`，无需改动 |
| `agent/shared/src/i18n/tools/web.rs` | 更新 `WebFetch` 中英文描述，提示其会返回清洗后的 Markdown、标题和链接 |

> 本期不改动 `web_search.rs`，不引入 Chromium/Playwright，不新增独立 `WebBrowse` tool。

---

## Task 1: 更新依赖

**Files:**
- Modify: `agent/features/tools/Cargo.toml`

- [ ] **Step 1: 在 `[dependencies]` 下新增 scraper 与 htmd**

```toml
scraper = "0.27"
htmd = "0.5"
```

- [ ] **Step 2: 运行 cargo check 确认依赖可解析**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath && cargo check -p tools
```

Expected: 依赖下载并编译通过（tools crate 原有代码不应报错）。

- [ ] **Step 3: Commit**

```bash
git add agent/features/tools/Cargo.toml
git commit -m "chore(tools): add scraper and htmd dependencies for WebFetch enhancement"
```

---

## Task 2: 扩展 WebFetchResult 类型

**Files:**
- Modify: `agent/shared/src/tool/types/web_fetch.rs`

- [ ] **Step 1: 为 WebFetchResult 新增 links 字段**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebFetchResult {
    pub url: String,
    pub title: String,
    pub content: String,
    pub truncated: bool,
    pub links: Vec<String>,
}
```

- [ ] **Step 2: 编译 share crate**

```bash
cargo check -p share
```

Expected: 通过。

- [ ] **Step 3: Commit**

```bash
git add agent/shared/src/tool/types/web_fetch.rs
git commit -m "feat(tools): add links field to WebFetchResult"
```

---

## Task 3: 编写 HTML 提取模块（先写测试，再实现）

**Files:**
- Create: `agent/features/tools/src/business/web_fetch/extract.rs`
- Create: `agent/features/tools/src/business/web_fetch/tests.rs`
- Modify: `agent/features/tools/src/business/web_fetch.rs`（新增 `mod extract; #[cfg(test)] mod tests;`）

### Task 3.1: 写失败的测试

- [ ] **Step 1: 创建 tests.rs，覆盖核心行为**

```rust
use super::extract::{extract_page, ExtractOptions};

#[test]
fn test_extract_title_and_markdown() {
    let html = r#"<!DOCTYPE html>
<html>
<head><title>Hello Page</title></head>
<body>
<nav>skip me</nav>
<main>
<h1>Hello</h1>
<p>World <a href="/relative">link</a></p>
</main>
<footer>foot</footer>
</body>
</html>"#;

    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 1_000_000,
        max_links: 50,
    };
    let result = extract_page(html, opts).unwrap();
    assert_eq!(result.title, "Hello Page");
    assert!(result.markdown.contains("# Hello"));
    assert!(result.markdown.contains("[link](https://example.com/relative)"));
    assert!(result.links.contains(&"https://example.com/relative".to_string()));
}

#[test]
fn test_fallback_to_body_when_no_main_or_article() {
    let html = r#"<html><head><title>No Main</title></head><body><p>content</p></body></html>"#;
    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 1_000_000,
        max_links: 50,
    };
    let result = extract_page(html, opts).unwrap();
    assert_eq!(result.title, "No Main");
    assert!(result.markdown.contains("content"));
}

#[test]
fn test_links_deduplicated_and_limited() {
    let html = r#"<html><body>
<a href="https://a.com/1">a</a>
<a href="https://a.com/1">a again</a>
<a href="https://a.com/2">b</a>
</body></html>"#;
    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 1_000_000,
        max_links: 2,
    };
    let result = extract_page(html, opts).unwrap();
    assert_eq!(result.links.len(), 2);
    assert!(result.links.contains(&"https://a.com/1".to_string()));
    assert!(result.links.contains(&"https://a.com/2".to_string()));
}

#[test]
fn test_exceeds_max_size_returns_error() {
    let html = "<html><body><p>hi</p></body></html>";
    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 10,
        max_links: 50,
    };
    assert!(extract_page(html, opts).is_err());
}

#[test]
fn test_non_html_content_not_parsed() {
    let text = "plain text content";
    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 1_000_000,
        max_links: 50,
    };
    let result = extract_page(text, opts).unwrap();
    assert!(result.markdown.contains("plain text content"));
    assert!(result.links.is_empty());
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test -p tools web_fetch::tests
```

Expected: 编译失败（`extract` module 不存在）。

### Task 3.2: 实现 extract 模块

- [ ] **Step 3: 创建 extract.rs**

```rust
use scraper::{Html, Selector};
use std::collections::HashSet;

pub struct ExtractOptions<'a> {
    pub base_url: &'a str,
    pub max_content_bytes: usize,
    pub max_links: usize,
}

pub struct ExtractedPage {
    pub title: String,
    pub markdown: String,
    pub links: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("HTML content exceeds maximum allowed size")]
    TooLarge,
    #[error("failed to parse base URL: {0}")]
    BaseUrl(#[from] url::ParseError),
}

pub fn extract_page(html: &str, opts: ExtractOptions<'_>) -> Result<ExtractedPage, ExtractError> {
    if html.len() > opts.max_content_bytes {
        return Err(ExtractError::TooLarge);
    }

    // If it doesn't look like HTML, return as-is without trying to parse.
    let trimmed = html.trim_start();
    if !trimmed.starts_with('<') {
        return Ok(ExtractedPage {
            title: String::new(),
            markdown: html.to_string(),
            links: Vec::new(),
        });
    }

    let document = Html::parse_document(html);
    let title = extract_title(&document);
    let links = extract_links(&document, opts.base_url, opts.max_links)?;
    let markdown = extract_content_markdown(&document)?;

    Ok(ExtractedPage {
        title,
        markdown,
        links,
    })
}

fn extract_title(document: &Html) -> String {
    let selector = Selector::parse("title").unwrap();
    document
        .select(&selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default()
}

fn extract_links(
    document: &Html,
    base_url: &str,
    max_links: usize,
) -> Result<Vec<String>, ExtractError> {
    let base = url::Url::parse(base_url)?;
    let selector = Selector::parse("a[href]").unwrap();
    let mut seen = HashSet::new();
    let mut links = Vec::new();

    for element in document.select(&selector) {
        if links.len() >= max_links {
            break;
        }
        if let Some(href) = element.value().attr("href") {
            // Skip anchors, javascript, mailto, tel, etc.
            if href.starts_with('#')
                || href.starts_with("javascript:")
                || href.starts_with("mailto:")
                || href.starts_with("tel:")
            {
                continue;
            }
            if let Ok(abs) = base.join(href) {
                let s = abs.to_string();
                if seen.insert(s.clone()) {
                    links.push(s);
                }
            }
        }
    }

    Ok(links)
}

fn extract_content_markdown(document: &Html) -> Result<String, ExtractError> {
    let selectors = [
        Selector::parse("main").unwrap(),
        Selector::parse("article").unwrap(),
        Selector::parse("body").unwrap(),
    ];

    let html_fragment = selectors
        .iter()
        .find_map(|sel| document.select(sel).next())
        .map(|el| el.html())
        .unwrap_or_else(|| document.html());

    let converter = htmd::HtmlToMarkdown::new();
    Ok(converter.convert(&html_fragment).unwrap_or(html_fragment))
}
```

> 注意：`htmd::HtmlToMarkdown` 的具体构造方式以实际 crate API 为准；如果 API 不同，在实现时按真实签名调整。

- [ ] **Step 4: 修改 web_fetch.rs 注册子模块**

在 `agent/features/tools/src/business/web_fetch.rs` 顶部新增：

```rust
mod extract;
#[cfg(test)]
mod tests;
```

- [ ] **Step 5: 运行测试确认通过**

```bash
cargo test -p tools web_fetch::tests
```

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add agent/features/tools/src/business/web_fetch/
git commit -m "feat(tools): add HTML extract module with title, markdown and links"
```

---

## Task 4: 改造 WebFetchTool 调用流程

**Files:**
- Modify: `agent/features/tools/src/business/web_fetch.rs`

### Task 4.1: 写集成测试（先失败）

- [ ] **Step 1: 在 tests.rs 中新增 WebFetchTool 调用测试**

```rust
use crate::api::{ToolExecutionContext, TypedTool};
use super::WebFetchTool;

#[tokio::test]
async fn test_web_fetch_parses_html() {
    // Use data URI-like local HTML by spinning up a tiny local server is too heavy;
    // instead we test through the extract module directly (Task 3).
    // Here we only verify the tool wires result fields correctly.
    let tool = WebFetchTool;
    let schema = tool.data_schema();
    assert!(schema.get("properties").unwrap().get("links").is_some());
}
```

> 由于 `WebFetchTool` 直接调用外部 `curl`，单元测试难以 Mock HTTP。集成测试留到 Task 5 通过本地文件/测试服务器验证。此处仅验证 schema 包含 `links`。

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test -p tools web_fetch
```

Expected: 当前 `data_schema()` 尚未包含 `links`，但通过 build.rs 生成的 schema 会自动反映 `WebFetchResult` 的 `links` 字段；如果未自动反映，检查 share crate 的 schema 生成逻辑。如果测试通过，说明 schema 已同步。

### Task 4.2: 修改 call 方法

- [ ] **Step 3: 在 `call` 方法中，curl 成功后按内容类型分流**

将原来直接构造 `WebFetchResult` 的部分替换为：

```rust
let body = String::from_utf8_lossy(&output.stdout);
let is_html = output
    .stdout
    .windows(5)
    .any(|w| w.eq_ignore_ascii_case(b"<html") || w.eq_ignore_ascii_case(b"<!doc"));

let extracted = if is_html {
    match extract::extract_page(
        &body,
        extract::ExtractOptions {
            base_url: url.as_str(),
            max_content_bytes: 2 * 1024 * 1024, // 2 MiB
            max_links: 50,
        },
    ) {
        Ok(e) => e,
        Err(err) => {
            return TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("HTML extraction failed: {err}"),
                    "data": { "url": url.as_str() }
                })
                .to_string(),
            );
        }
    }
} else {
    extract::ExtractedPage {
        title: String::new(),
        markdown: body.to_string(),
        links: Vec::new(),
    }
};

// Apply truncation to markdown content
let content = if extracted.markdown.len() > max_chars {
    let truncated = share::string_idx::slice_head(&extracted.markdown, max_chars);
    format!(
        "{}...\n\n[truncated, showing first {} chars of {} total]",
        truncated,
        truncated.chars().count(),
        extracted.markdown.chars().count()
    )
} else {
    extracted.markdown.clone()
};

TypedToolResult::success(
    content.clone(),
    WebFetchResult {
        url: url.to_string(),
        title: extracted.title,
        content,
        truncated: extracted.markdown.len() > max_chars,
        links: extracted.links,
    },
)
```

并移除原实现中直接 `body.to_string()` 的 `WebFetchResult` 构造。

- [ ] **Step 4: 运行 tools crate 测试与 clippy**

```bash
cargo test -p tools web_fetch
cargo clippy -p tools --all-targets
```

Expected: PASS / 无新警告。

- [ ] **Step 5: Commit**

```bash
git add agent/features/tools/src/business/web_fetch.rs
git commit -m "feat(tools): wire HTML extraction into WebFetchTool"
```

---

## Task 5: 更新 i18n 描述

**Files:**
- Modify: `agent/shared/src/i18n/tools/web.rs`

- [ ] **Step 1: 读取当前 web_fetch 中英文描述**

```bash
grep -n "web_fetch" agent/shared/src/i18n/tools/web.rs
```

- [ ] **Step 2: 更新描述，说明会返回 Markdown、title、links**

示例（以实际函数签名准）：

```rust
pub fn web_fetch(lang: &str) -> &'static str {
    match lang {
        "zh" => "通过 HTTP GET 获取 URL 内容。只读。对于 HTML 页面，会提取标题、将正文转换为 Markdown 并列出页面链接；大内容会被截断。",
        _ => "Fetches content from a URL via HTTP GET. Read-only. For HTML pages, extracts the title, converts the body to Markdown, and lists page links. Large content may be truncated.",
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo check -p share
```

- [ ] **Step 4: Commit**

```bash
git add agent/shared/src/i18n/tools/web.rs
git commit -m "feat(tools): update WebFetch i18n description for markdown/title/links"
```

---

## Task 6: 端到端验证

**Files:** 不涉及文件修改，仅运行命令。

- [ ] **Step 1: 启动本地静态文件服务器提供测试 HTML**

```bash
mkdir -p /tmp/webfetch-test && cat > /tmp/webfetch-test/page.html <<'EOF'
<!DOCTYPE html>
<html>
<head><title>Test Page</title></head>
<body>
<main>
<h1>Hello WebFetch</h1>
<p>This is <a href="/next.html">next</a> and <a href="https://example.com/out">external</a>.</p>
</main>
</body>
</html>
EOF
cd /tmp/webfetch-test && python3 -m http.server 8765 &
SERVER_PID=$!
sleep 1
```

- [ ] **Step 2: 使用 aemeath CLI 调用 WebFetch**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
echo '{"url":"http://localhost:8765/page.html"}' | AEMEATH_VERSION= RUST_LOG= cargo run -- -q --tool WebFetch
```

Expected: 输出包含 `title: "Test Page"`、`content` 中有 `# Hello WebFetch`，`links` 包含 `http://localhost:8765/next.html` 与 `https://example.com/out`。

- [ ] **Step 3: 停止测试服务器**

```bash
kill $SERVER_PID
```

- [ ] **Step 4: 全量验证门禁**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
cargo test -p tools
cargo clippy -p tools --all-targets -- -D warnings
cargo fmt --check
```

Expected: 全部通过。

- [ ] **Step 5: Commit any final fixes**

```bash
git add -A
git commit -m "test(tools): add end-to-end verification for WebFetch HTML extraction" || true
```

---

## 自我审查

**1. Spec coverage（Issue #529 验收标准）:**
- 梳理当前 WebFetch 输入/输出、错误处理、截断策略和安全边界 ✅ Task 1-4 文档化在代码与测试里
- 对比 crawl4ai 关键能力，列出可复用/可借鉴点 ✅ 已在 issue comment 中完成
- 给出是否增强 WebFetch、拆新工具、或保持轻量的设计建议 ✅ 本期选择增强 WebFetch 并保持轻量
- 明确验证方式和潜在安全风险 ✅ Task 6 与 Task 3/4 的安全边界

**2. Placeholder scan:**
- 无 "TBD"/"TODO"/"。
- 每个步骤包含实际代码或命令。
- `htmd` API 以实际 crate 为准的提示已标注，实现时按真实签名调整。

**3. Type consistency:**
- `WebFetchResult.links` 在 Task 2 定义，Task 4 填充，Task 5 的 schema 自动生成。
- `extract::ExtractOptions` 与 `extract::ExtractedPage` 在 Task 3 定义并在 Task 4 使用，字段一致。

---

## 执行方式选择

**Plan complete and saved to `docs/superpowers/plans/2026-06-28-enhance-web-fetch-html-to-markdown.md`.**

Two execution options:

1. **Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

> **Execution deviation note:** The original Task 6 proposed launching a local HTTP server and invoking the aemeath CLI with `--tool WebFetch`. The CLI does not expose a `--tool` flag; end-to-end verification through the agent loop would require a configured LLM provider and API key. Therefore Task 6 was completed by running the full verification gate instead: `cargo test -p tools`, `cargo clippy -p tools --all-targets -- -D warnings`, and `cargo fmt --check`. All passed.

Which approach?
