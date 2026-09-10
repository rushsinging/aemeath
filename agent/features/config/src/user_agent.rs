//! Config-owned Provider User-Agent 四级 resolver。
//!
//! 严格遵循 [`specs/3.9-config-compat.md §3.9.4`](../../../../specs/3.9-config-compat.md) 与
//! [`docs/design/02-modules/config/02-provider-catalog-and-connect.md §5`](../../../../docs/design/02-modules/config/02-provider-catalog-and-connect.md)：
//!
//! 1. Provider 专属配置 `models.providers.<source>.userAgent`；
//! 2. Provider Catalog 中对应已核验官方 SDK 的默认 UA；
//! 3. 全局配置 `api.user_agent`；
//! 4. 全局内置默认 UA `Aemeath/<version> cli <os>/<os-version>/<arch>`。
//!
//! 任何空白或非法 HeaderValue 必须跳过该级并继续回退；系统版本不可用时降级为
//! `Aemeath/<version> cli <os>/<arch>`。UA 不得泄漏 API Key、session id、路径。

use http::HeaderValue;

use crate::ports::SystemInformation;

/// Resolver 输入。Config domain 把所有上游字符串归一为 `Option<&str>` / `HeaderValue`
/// 后传入；本函数不再二次解析 JSON / 文件。
#[derive(Debug, Clone)]
pub struct ProviderUserAgentInputs<'a> {
    /// `models.providers.<source>.userAgent` 经 [`crate::catalog`] / Config 归一化后的值。
    pub provider_user_agent: Option<&'a str>,
    /// Catalog 已核验官方 SDK UA。**NEVER** 来自 Catalog 之外的代码路径。
    pub catalog_official_sdk_user_agent: Option<HeaderValue>,
    /// 全局配置 `api.user_agent`。
    pub global_user_agent: Option<&'a str>,
    /// 平台信息。
    pub system: SystemInformation,
    /// Aemeath 编译期版本。
    pub version: &'a str,
}

/// 把字符串解析为 [`HeaderValue`]，失败时返回 `None`。
///
/// 用于拒绝空白字符串、含控制字符、含非 ASCII 字符的输入；调用方据此继续回退
/// 到下一级。`HeaderValue::from_str` 在内部允许非 ASCII 字节通过，但其 `to_str`
/// 会在非 ASCII 路径返回错误，故本函数额外要求 `to_str().is_ok()` 才算合法。
fn parse_header_value(raw: &str) -> Option<HeaderValue> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let header = HeaderValue::from_str(trimmed).ok()?;
    if header.to_str().is_err() {
        return None;
    }
    Some(header)
}

/// 计算全局默认 UA。
///
/// - `system.os_version` 为 `None` 或空白时降级为 `<os>/<arch>` 双段；
/// - 所有动态片段先经 [`sanitize_segment`] 清洗：ASCII 控制字符与非 ASCII
///   字节都替换为 `-`，整个 fragment 清洗后为空时回退到 `fallback`；
/// - 最后再做一次 `HeaderValue::from_str` 兜底校验，失败时降级到最小安全形式。
pub fn build_global_default_user_agent(system: &SystemInformation, version: &str) -> HeaderValue {
    let os = sanitize_segment(&system.os_name, "unknown-os");
    let arch = sanitize_segment(&system.arch, "unknown-arch");
    let version_segment = sanitize_segment(version, "0.0.0");

    let ua = match system
        .os_version
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(os_version) => {
            let os_version_segment = sanitize_segment(os_version, "unknown-version");
            format!("Aemeath/{version_segment} cli {os}/{os_version_segment}/{arch}")
        }
        None => format!("Aemeath/{version_segment} cli {os}/{arch}"),
    };

    // 构造期间已清洗；构造 HeaderValue 仍做一次校验，失败则降级为最小安全形式。
    HeaderValue::from_str(&ua)
        .unwrap_or_else(|_| HeaderValue::from_static("Aemeath/0.0.0 cli unknown-os/unknown-arch"))
}

/// 把一个动态片段清洗为 HeaderValue 安全的字符串：
/// - 去除前后空白；
/// - 替换 ASCII 控制字符（< 0x20 或 0x7F）为 `-`；
/// - **替换任何非 ASCII 字符**（含中日韩）为 `-`，避免 `HeaderValue::to_str`
///   在下游 Provider 失败而被迫 fallback 到占位字符串；
/// - 若清洗后为空，使用 `fallback`。
fn sanitize_segment(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    let mut out = String::with_capacity(trimmed.len());
    let mut saw_kept = false;
    for ch in trimmed.chars() {
        if ch.is_ascii() {
            let byte = ch as u8;
            if byte.is_ascii_control() {
                out.push('-');
            } else {
                out.push(ch);
                saw_kept = true;
            }
        } else {
            // 非 ASCII（含中日韩等扩展字符）替换为 `-`，保证片段在
            // `HeaderValue::from_str` / `to_str` 之后仍可被 Provider 接收。
            out.push('-');
        }
    }
    let final_str = out.trim().to_string();
    if saw_kept && !final_str.is_empty() {
        final_str
    } else {
        fallback.to_string()
    }
}

/// Provider UA 解析的原始输入请求。
///
/// 这是「调用方能看到什么」的最小集合：Provider 专属覆盖、用于查询 Catalog
/// 官方 SDK UA 的 source / driver 键、以及全局配置 UA。所有调用方（Runtime main
/// run、派生 run、Runtime 全局回退、Connect probe）都**MUST**经
/// [`assemble_provider_user_agent_inputs`] 装配输入，**NEVER**各自拼装
/// [`ProviderUserAgentInputs`]：同一 resolver 收到不同输入集合会让 probe 与正式
/// 请求的 UA 漂移。
#[derive(Debug, Clone, Copy)]
pub struct ProviderUserAgentRequest<'a> {
    /// `models.providers.<source>.userAgent` 原始值（未归一化）。
    pub provider_user_agent: Option<&'a str>,
    /// Catalog `source` key；用于查询该 source 的官方 SDK UA。
    pub source_key: Option<&'a str>,
    /// `source_key` 缺失时的 driver 兼容查询键。
    pub driver: Option<&'a str>,
    /// 全局配置 `api.user_agent` 原始值（未归一化）。
    pub global_user_agent: Option<&'a str>,
}

/// 唯一装配入口：把原始请求 + 环境信息装配为 resolver 输入。
///
/// - Provider 专属与全局 UA 先做空白归一：空白等同未配置，继续回退；
/// - Catalog 官方 SDK UA 按 `source_key` 优先、`driver` 兜底查询；无证据时保持
///   `None`，**NEVER** 伪造数值。
pub fn assemble_provider_user_agent_inputs<'a>(
    request: ProviderUserAgentRequest<'a>,
    system: SystemInformation,
    version: &'a str,
) -> ProviderUserAgentInputs<'a> {
    let catalog_official_sdk_user_agent = request
        .source_key
        .and_then(crate::catalog::find_by_source)
        .or_else(|| request.driver.and_then(crate::catalog::find_by_driver))
        .and_then(|entry| entry.official_sdk_user_agent.as_ref())
        .map(|official| official.value.clone());

    ProviderUserAgentInputs {
        provider_user_agent: non_blank(request.provider_user_agent),
        catalog_official_sdk_user_agent,
        global_user_agent: non_blank(request.global_user_agent),
        system,
        version,
    }
}

/// 空白字符串等同未配置。
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// 计算 Provider 请求最终 UA 并以 `&str` 形式返回。
///
/// 等价于 [`resolve_provider_user_agent`]，仅返回字符串便于断言。
pub fn resolve_provider_user_agent_str(inputs: ProviderUserAgentInputs<'_>) -> String {
    let header = resolve_provider_user_agent(inputs);
    header
        .to_str()
        .map(str::to_string)
        .unwrap_or_else(|_| "Aemeath/0.0.0 cli unknown-os/unknown-arch".to_string())
}

/// 计算 Provider 请求最终 UA 并以 [`HeaderValue`] 形式返回。
///
/// 严格四级优先级，每级遇空白或非法 HeaderValue 时跳过该级；系统版本不可用时
/// 降级为 `<os>/<arch>` 双段。Catalog 官方 SDK UA 直接使用 `Option<HeaderValue>`：
/// 构造方需保证它代表通过 `to_str()` 的可见 ASCII；构造入口（[`crate::catalog`]）
/// 在构造期已做 HeaderValue 安全校验。
pub fn resolve_provider_user_agent(inputs: ProviderUserAgentInputs<'_>) -> HeaderValue {
    // 1. Provider 专属
    if let Some(raw) = inputs.provider_user_agent.and_then(parse_header_value) {
        return raw;
    }

    // 2. Catalog 官方 SDK 默认
    if let Some(value) = inputs.catalog_official_sdk_user_agent {
        if value.to_str().is_ok() {
            return value;
        }
    }

    // 3. 全局配置
    if let Some(raw) = inputs.global_user_agent.and_then(parse_header_value) {
        return raw;
    }

    // 4. 全局默认
    build_global_default_user_agent(&inputs.system, inputs.version)
}
