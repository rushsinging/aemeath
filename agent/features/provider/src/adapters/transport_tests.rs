//! `transport.rs` 行为测试：loopback endpoint 的 HTTP client 必须禁用代理。
//!
//! reqwest 默认在 macOS 读取系统代理（`macos-system-configuration`
//! feature），开启代理软件的机器上发往本地 mock server 的请求会被
//! 代理截获并偶发返回 502 之类伪响应，导致依赖本地 mock 的契约测试
//! 偶发失败；因此 loopback endpoint 的 client 构造时彻底禁用代理。

use super::{endpoint_uses_loopback, http_builder_for_endpoint};

#[test]
fn endpoint_uses_loopback_matches_only_loopback_hosts() {
    assert!(endpoint_uses_loopback(Some("http://127.0.0.1:11434")));
    assert!(endpoint_uses_loopback(Some("http://localhost:11434")));
    assert!(endpoint_uses_loopback(Some("http://[::1]:11434")));
    assert!(endpoint_uses_loopback(Some("https://localhost/api")));

    assert!(!endpoint_uses_loopback(Some("https://api.anthropic.com")));
    assert!(!endpoint_uses_loopback(Some("http://192.168.1.10:8080")));
    // 无 base_url（各 driver 默认 endpoint 均非 loopback）保守不启用。
    assert!(!endpoint_uses_loopback(None));
    // 无法解析的输入保守不启用，不改变现有行为。
    assert!(!endpoint_uses_loopback(Some("localhost:11434")));
    assert!(!endpoint_uses_loopback(Some("not a url")));
}

#[test]
fn http_builder_sets_connect_timeout_for_all_endpoints() {
    for endpoint in [
        Some("http://127.0.0.1:1"),
        Some("https://api.anthropic.com"),
        None,
    ] {
        let debug = format!("{:?}", http_builder_for_endpoint(endpoint));
        assert!(debug.contains("connect_timeout"), "{debug}");
    }
}

// `http_builder_for_endpoint` 对 loopback endpoint 调用 `no_proxy()` 的
// 行为不在此处断言：`ClientBuilder` 的 Debug 不打印代理字段，而 env 注入
// 会污染并行测试。该决策由 `endpoint_uses_loopback` 判定单测 + 真实系统
// 代理环境（macOS scutil 代理开启）下 mock 契约测试（如 anthropic 429
// 终结测试）的反复全绿共同锁定：移除 `no_proxy()` 会让这类测试在开启
// 代理的开发机上重新出现代理伪响应（502）导致的偶发失败。
