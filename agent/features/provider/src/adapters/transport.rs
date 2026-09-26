//! 不可变 Provider Transport：跨 invocation 共享的连接与认证事实。
//!
//! Transport 构造后不可变，生命周期可覆盖多个 Run；不保存 current model、
//! current reasoning 或 current max tokens（那些属于 `InvocationScopeData`）。

use std::time::Duration;

use crate::domain::capability::ProviderDriverKind;

/// 唯一标识一个不可变 transport 的构造事实集合。
///
/// key 至少覆盖 provider endpoint、认证域与 driver identity；model 只有在
/// 它确实决定 transport 时才允许进入 key——当前没有任何 driver 满足该条件，
/// 因此 model / max_tokens / reasoning **MUST NOT** 出现在 key 中。
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct TransportKey {
    /// Driver 身份（协议家族由 driver 字符串决定）。
    pub(crate) driver_kind: ProviderDriverKind,
    /// API style hint（如 `"responses"`）；决定请求路径，属于 transport 事实。
    pub(crate) api_style: Option<String>,
    /// 请求 endpoint 的 base URL（driver 构造时的原始输入；driver 内部
    /// 规范化由同一 `driver_kind` 决定，故原始字符串已足以区分 endpoint）。
    pub(crate) base_url: Option<String>,
    /// 认证域：当前以 API key 全文标识；凭证轮换即产生新 key、新 transport。
    pub(crate) api_key: String,
    /// Run-frozen HTTP User-Agent；变化即产生新 transport。
    pub(crate) user_agent: String,
    /// 请求超时秒数；属于 transport 构造参数。
    pub(crate) timeout_secs: u64,
}

/// 构造后不可变的连接事实：HTTP connection pool + 单调递增诊断 id。
///
/// id 仅用于日志与测试断言"同一 transport 真相"，不参与任何协议行为。
pub(crate) struct ProviderTransport {
    id: u64,
    http: reqwest::Client,
}

impl ProviderTransport {
    pub(crate) fn new(id: u64, base_url: Option<&str>) -> Self {
        Self {
            id,
            http: build_http_client_for_endpoint(base_url),
        }
    }

    /// 诊断 id：同一 pool 内单调递增，用于识别连接复用真相。
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// 共享的 HTTP client（内部为 Arc，clone 廉价）。
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }
}

/// 判断 endpoint 是否指向 loopback（localhost / 127.0.0.1 / ::1）。
///
/// reqwest 默认在 macOS 读取系统代理（`macos-system-configuration`），
/// 开启代理软件的机器上发往本地服务的请求会被代理截获并偶发返回
/// 502 等伪响应；loopback 流量走代理几乎从不是调用方意图，因此对
/// loopback endpoint 强制绕过代理。无法解析的输入保守返回 false，
/// 不改变现有行为。
pub(crate) fn endpoint_uses_loopback(base_url: Option<&str>) -> bool {
    let raw_url = match base_url {
        Some(url) => url,
        None => return false,
    };
    match reqwest::Url::parse(raw_url) {
        Ok(url) => {
            let host = url.host_str().unwrap_or_default();
            // IPv6 字面量的 host_str 可能带方括号，统一剥掉再比较。
            let host = host.trim_start_matches('[').trim_end_matches(']');
            matches!(host, "localhost" | "127.0.0.1" | "::1")
        }
        Err(_) => false,
    }
}

/// 按当前所有 driver 一致的构造参数建立 HTTP client builder；
/// loopback endpoint 额外禁用系统/环境代理。
pub(crate) fn http_builder_for_endpoint(base_url: Option<&str>) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(crate::CONNECT_TIMEOUT_SECS));
    if endpoint_uses_loopback(base_url) {
        // loopback 流量走代理几乎从不是调用方意图；reqwest 默认在
        // macOS 读取系统代理，会把本地请求交给代理并偶发返回 502
        // 之类伪响应，因此对 loopback endpoint 彻底禁用代理。
        builder.no_proxy()
    } else {
        builder
    }
}

/// 按当前所有 driver 一致的构造参数建立 HTTP client。
pub(crate) fn build_http_client_for_endpoint(base_url: Option<&str>) -> reqwest::Client {
    http_builder_for_endpoint(base_url)
        .build()
        .expect("failed to create HTTP client")
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
