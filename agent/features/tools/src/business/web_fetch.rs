use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use tokio::process::Command;
use url::Url;

pub struct WebFetchTool;

/// Validate a URL against SSRF attacks.
///
/// - Only http/https schemes allowed.
/// - Block private IPs, loopback, link-local, multicast, broadcast.
/// - Block common cloud metadata endpoints.
fn validate_url(raw_url: &str) -> Result<Url, String> {
    let url = Url::parse(raw_url).map_err(|e| format!("invalid URL: {e}"))?;

    // Only allow http/https
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "scheme '{}' is not allowed. Only http and https are supported.",
                other
            ))
        }
    }

    // Resolve host and check against private ranges
    let host = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;

    // Block known cloud metadata hosts
    if host == "169.254.169.254" || host.ends_with("169.254.169.254") {
        return Err("access to cloud metadata service is blocked".to_string());
    }

    // Try to parse as IP address (covers both IPv4 and IPv6)
    if let Ok(ip) = host.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(v4) => {
                if v4.is_private() {
                    return Err(format!("private IP address {} is not allowed", v4));
                }
                if v4.is_loopback() {
                    return Err(format!("loopback address {} is not allowed", v4));
                }
                if v4.is_link_local() {
                    return Err(format!("link-local address {} is not allowed", v4));
                }
                if v4.is_multicast() {
                    return Err(format!("multicast address {} is not allowed", v4));
                }
                if v4.is_broadcast() {
                    return Err(format!("broadcast address {} is not allowed", v4));
                }
                // 0.0.0.0
                if v4 == Ipv4Addr::UNSPECIFIED {
                    return Err("unspecified address 0.0.0.0 is not allowed".to_string());
                }
            }
            IpAddr::V6(v6) => {
                if v6.is_loopback() {
                    return Err(format!("loopback address {} is not allowed", v6));
                }
                if v6.is_multicast() {
                    return Err(format!("multicast address {} is not allowed", v6));
                }
                if v6 == Ipv6Addr::UNSPECIFIED {
                    return Err("unspecified address :: is not allowed".to_string());
                }
                // IPv6 link-local: fe80::/10
                if v6.segments()[0] & 0xffc0 == 0xfe80 {
                    return Err(format!("link-local address {} is not allowed", v6));
                }
                // IPv6 unique-local (fc00::/7) — equivalent to private
                if v6.segments()[0] & 0xfe00 == 0xfc00 {
                    return Err(format!("unique-local address {} is not allowed", v6));
                }
            }
        }
    }

    // Block well-known localhost hostnames
    if host == "localhost" || host.ends_with(".localhost") || host == "localtest.me" {
        return Err(format!(
            "hostname '{}' resolves to localhost and is not allowed",
            host
        ));
    }

    Ok(url)
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        "Fetches content from a specified URL via HTTP GET.\n\nUsage:\n- The URL must be a fully-formed valid URL\n- HTTP URLs will be automatically upgraded to HTTPS\n- This tool is read-only and does not modify any files\n- Results may be truncated if the content is very large\n- For GitHub URLs, prefer using the gh CLI via Bash instead (e.g. gh pr view, gh issue view)"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default 30000)"
                }
            },
            "required": ["url"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: serde_json::Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let raw_url = match input.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => {
                return ToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": "missing required parameter: url",
                        "data": null
                    })
                    .to_string(),
                )
            }
        };

        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30_000);

        // Validate URL and upgrade http to https
        let mut url = match validate_url(raw_url) {
            Ok(u) => u,
            Err(e) => {
                return ToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!("URL rejected: {e}"),
                        "data": { "url": raw_url }
                    })
                    .to_string(),
                )
            }
        };

        // Upgrade http to https
        if url.scheme() == "http" {
            url.set_scheme("https").ok();
        }

        // Use curl as it's universally available and handles redirects/TLS
        let result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            Command::new("curl")
                .args([
                    "-sL", // silent, follow redirects
                    "--max-time",
                    &(timeout_ms / 1000).max(5).to_string(),
                    "-A",
                    "aemeath/0.1.0",
                    // Limit redirect count to reduce SSRF surface
                    "--max-redirs",
                    "5",
                    url.as_str(),
                ])
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let body = String::from_utf8_lossy(&output.stdout);
                    // Truncate very large responses safely
                    let max_chars = 50_000;
                    if body.len() > max_chars {
                        let truncated = share::string_idx::slice_head(&body, max_chars);
                        ToolResult::success(serde_json::json!({
                            "status": "success",
                            "message": format!("Fetched {} (truncated, showing first {} chars of {} total)", url, truncated.chars().count(), body.chars().count()),
                            "data": {
                                "url": url.as_str(),
                                "content": format!("{}...\n\n[truncated, showing first {} chars of {} total]", truncated, truncated.chars().count(), body.chars().count())
                            }
                        }).to_string())
                    } else {
                        ToolResult::success(
                            serde_json::json!({
                                "status": "success",
                                "message": format!("Fetched {}", url),
                                "data": {
                                    "url": url.as_str(),
                                    "content": body.to_string()
                                }
                            })
                            .to_string(),
                        )
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    ToolResult::error(
                        serde_json::json!({
                            "status": "error",
                            "message": format!("fetch failed: {stderr}"),
                            "data": { "url": url.as_str() }
                        })
                        .to_string(),
                    )
                }
            }
            Ok(Err(e)) => ToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("failed to execute curl: {e}"),
                    "data": { "url": url.as_str() }
                })
                .to_string(),
            ),
            Err(_) => ToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("request timed out after {timeout_ms}ms"),
                    "data": { "url": url.as_str() }
                })
                .to_string(),
            ),
        }
    }
}
