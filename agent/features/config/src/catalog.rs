//! Config-owned Provider Catalog.
//!
//! ## 设计目标
//!
//! - 提供 Config domain 唯一的内置 Provider 默认值集合（base URL、推荐模型、
//!   API key env、官方 SDK User-Agent 证据）；
//! - 固定 source 名称（`Anthropic`、`OpenAI` 等）作为 `models.providers` 的稳定 key；
//! - 拒绝重复 source、非法 URL、非法 HeaderValue 与无效模型窗口；同一 runtime
//!   driver 可服务多个拥有不同 endpoint 的内置 Provider 配置；
//! - **NEVER** 依赖 provider crate；driver 字符串与 Config domain `driver_env`
//!   单一真相一致；
//! - 官方 SDK User-Agent 没有可靠证据时**必须** `None`，绝不猜测。
//!
//! ## 目录稳定性
//!
//! Catalog 是 Config domain 的静态数据集，新条目必须同步增加：
//! 1. provider 实现（参见 `3.6-provider.md`）；
//! 2. Catalog 条目与证据元数据；
//! 3. 本文件的契约测试。
//!
//! 不允许把另一份默认 base URL / 推荐模型 / API key env 写到 provider adapter，
//! 违反 `3.6-provider.md §2` 与 `3.9-config-compat.md §3.9.4`。
//!
//! ## 证据要求
//!
//! `default_endpoint` 与 `recommended_models` 必须有可核验证据，缺一不可：
//! - 没有证据时**必须**把字段留空（`None` / `&[]`）；
//! - 禁止把“待复核/示例”伪装为正式默认；
//! - 当前已核验 Anthropic / OpenAI / Zhipu / Minimax / Mimo / DeepSeek 的 endpoint
//!   与推荐模型，各自记录 evidence 来源与核验日期；LiteLLM / Agnes / Ollama 没有
//!   可核验证据，继续要求用户显式填写；Volcengine 同样缺证据，因此不进入 Catalog。

/// Config-owned 固定 source 名称。
///
/// TUI 只展示 Catalog DTO，禁止自行拼接 source。`ProviderSource::new` 仅允许
/// 与 [`PROVIDER_CATALOG`] 中已注册的固定名称相等，运行时输入的 source key 必须
/// 在配置层校验后进入 Config BC。
/// Provider source 稳定 key。Catalog 条目为 `Borrowed` 静态字面量；
/// 完全自定义 Provider 经 [`ProviderSource::new_owned`] 持有运行时名称。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderSource(std::borrow::Cow<'static, str>);

impl ProviderSource {
    pub const fn new(value: &'static str) -> Self {
        Self(std::borrow::Cow::Borrowed(value))
    }

    /// 持有运行时自定义 source 名称（Connect 向导"完全自定义"路径）。
    pub fn new_owned(value: String) -> Self {
        Self(std::borrow::Cow::Owned(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Config-owned driver 字符串类型。driver 是 Config domain 的内部枚举（与
/// `provider::ProviderDriverKind` 同步但**不依赖** provider crate）。driver 字符串
/// 与 Config domain `driver_env::driver_api_key_env_name` 单一真相一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DriverId(&'static str);

impl DriverId {
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

use http::HeaderValue;

/// 推荐模型条目。
///
/// 字段说明：
/// - `model_id` / `context_window` / `max_tokens` 是基本窗口；
/// - `evidence_url` / `verified_at` 必须配套填写，保证 `recommended_models` 一旦
///   非空就携带可核验证据，避免悄悄引入仓库 fixture / adapter 默认。
///
/// 已核验条目可提供推荐模型；无法核验的条目由 Connect 引导用户填写。
/// 新增条目时必须配套契约测试 `catalog_recommended_models_carry_evidence_metadata_when_present`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecommendedModel {
    pub model_id: &'static str,
    pub context_window: usize,
    pub max_tokens: u32,
    /// 证据链接，**必须**为 `https://...`（非空）。
    pub evidence_url: &'static str,
    /// 核验日期，**必须**晚于 1970-01-01。
    pub verified_at: chrono::NaiveDate,
}

/// Catalog 中的默认 base URL 值对象。
///
/// 字段说明：
/// - `url` 是真正的 base URL；
/// - `evidence_url` / `verified_at` 是证据元数据，**必须**为 `https://...` 与
///   非 1970-01-01 之前的合法日期。
///
/// 已核验条目可提供 `default_endpoint`；无法核验时保持 `None` 并由 Connect 引导用户填写。
/// 新增证据时必须配套契约测试 `catalog_default_endpoint_evidence_is_complete_when_present`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultEndpoint {
    pub url: &'static str,
    /// 证据链接，**必须**为 `https://...`（非空）。
    pub evidence_url: &'static str,
    /// 核验日期，**必须**晚于 1970-01-01。
    pub verified_at: chrono::NaiveDate,
}

/// 官方 SDK User-Agent 证据值。
///
/// 仅当 [`PROVIDER_CATALOG`] 的构造路径能验证以下事实时才能存在：
/// 1. 该字符串与某一官方 SDK 客户端（HTTP 客户端或 CLI）报告的真实 UA 严格相等；
/// 2. 已记录可核验的 SDK 名称、版本、证据链接；
///
/// 缺少任何一项或证据不可信时 [`ProviderCatalogEntry::official_sdk_user_agent`]
/// 必须为 `None`。**NEVER** 凭空拼接 `Mozilla/5.0 (...)` 或
/// `curl/x.y.z` 等通用客户端字符串冒充官方 UA。
#[derive(Debug, Clone)]
pub struct OfficialSdkUserAgent {
    pub sdk_name: &'static str,
    pub sdk_version: &'static str,
    /// 官方 SDK / CLI 客户端实际发送的 `User-Agent` 字面量。
    ///
    /// 保持 `&'static str` 而非 `HeaderValue`：`HeaderValue` 内部带原子引用计数
    /// （interior mutability），无法进入 `static PROVIDER_CATALOG`。发送前由
    /// [`OfficialSdkUserAgent::header_value`] 解析。
    pub value: &'static str,
    pub evidence_url: &'static str,
    pub verified_at: chrono::NaiveDate,
}

impl OfficialSdkUserAgent {
    /// 把已核验的 UA 字面量解析为可发送的 [`HeaderValue`]。
    ///
    /// 非可见 ASCII 或非法 HeaderValue 时返回 `None`，由 UA resolver 继续回退到
    /// 下一级；**NEVER** 在这里 panic——Catalog 数据不得让运行时崩溃。
    pub fn header_value(&self) -> Option<HeaderValue> {
        let value = HeaderValue::from_str(self.value).ok()?;
        value.to_str().is_ok().then_some(value)
    }
}

/// 单个 Provider 的 Catalog 条目。
///
/// 默认 base URL（`default_endpoint`）与推荐模型（`recommended_models`）都允许为
/// `None` / `&[]`，由 Connect 引导用户在缺失证据的条目中显式填写。
#[derive(Debug, Clone)]
pub struct ProviderCatalogEntry {
    pub source: ProviderSource,
    pub driver: DriverId,
    pub default_endpoint: Option<DefaultEndpoint>,
    pub recommended_models: &'static [RecommendedModel],
    pub api_key_hint: Option<&'static str>,
    pub official_sdk_user_agent: Option<OfficialSdkUserAgent>,
}

// 静态 Catalog 数据集（PROVIDER_CATALOG、Provider 条目、推荐模型与证据元数据）
// 已按 #1146 归位 `crate::domain::constants`；此处保留 crate 根可直达的 re-export。
pub use crate::domain::constants::PROVIDER_CATALOG;

// ---------------------------------------------------------------------------
// 查询 API
// ---------------------------------------------------------------------------

/// 按固定 source 名称查询 Catalog 条目。区分大小写（source 是稳定 key）。
pub fn find_by_source(source: &str) -> Option<&'static ProviderCatalogEntry> {
    PROVIDER_CATALOG
        .iter()
        .find(|entry| entry.source.as_str() == source)
}

/// 按 driver 字符串查询 Catalog 条目。大小写不敏感（driver 来自用户 JSON）。
///
/// 实现要点：`str::eq_ignore_ascii_case` 在比较时已对两边逐字节进行 ASCII 折叠，
/// 不需要先把入参降为小写再做比较，避免一次 `String` 分配。
pub fn find_by_driver(driver: &str) -> Option<&'static ProviderCatalogEntry> {
    PROVIDER_CATALOG
        .iter()
        .find(|entry| entry.driver.as_str().eq_ignore_ascii_case(driver))
}

/// 判断 driver 字符串是否属于 Catalog 已知 driver。
pub fn is_known_driver(driver: &str) -> bool {
    find_by_driver(driver).is_some()
}

/// 按 driver 查询对应的固定 source 名称（大小写不敏感）。
pub fn provider_source_for_driver(driver: &str) -> Option<ProviderSource> {
    find_by_driver(driver).map(|entry| entry.source.clone())
}

/// 返回 Catalog 中指定 source 的默认 base URL（字符串切片视图）。
///
/// `default_endpoint` 为 `None` 时返回 `None`；提供 `default_endpoint_url` 作为
/// 兼容 getter，内部直接映射 `entry.default_endpoint.as_ref().map(|e| e.url)`。
pub fn default_endpoint_url(source: &str) -> Option<&'static str> {
    find_by_source(source)
        .and_then(|entry| entry.default_endpoint.as_ref().map(|endpoint| endpoint.url))
}

/// 返回 Catalog 中指定 driver 的官方 SDK UA 证据值（无可靠证据时为 `None`）。
pub fn official_sdk_user_agent(driver: &str) -> Option<&'static OfficialSdkUserAgent> {
    find_by_driver(driver).and_then(|entry| entry.official_sdk_user_agent.as_ref())
}

/// 返回 Catalog 中指定 driver 的 API key 环境变量名。
///
/// 该函数是 Config domain `driver_env::driver_api_key_env_name` 的薄封装，强制
/// 与单一真相保持一致；任何新增 driver 必须先在 `driver_env.rs` 中注册。
pub fn api_key_env_name(driver: &str) -> Option<&'static str> {
    share::config::domain::driver_env::driver_api_key_env_name(driver)
}

// ---------------------------------------------------------------------------
// 启动期不变量校验
// ---------------------------------------------------------------------------

/// 在首次访问时执行 Catalog 不变量校验。
///
/// 返回错误（首次调用）后该不变量被锁定，重复调用返回 `Ok(())`。该函数用于
/// 启动期断言与 Catalog 测试目的；runtime 路径不会显式调用它（Catalog 是
/// 静态不变数据）。当前契约允许 `recommended_models` 为空（无核验证据）。
#[doc(hidden)]
pub fn static_assert_catalog_invariants() -> Result<(), &'static str> {
    match crate::domain::constants::VALIDATION_CACHE
        .get_or_init(|| check_unique_sources().and(check_recommended_models()))
    {
        Ok(()) => Ok(()),
        Err(_) => Err("Catalog 不变量校验失败"),
    }
}

fn check_unique_sources() -> Result<(), &'static str> {
    use std::collections::HashSet;
    let mut seen: HashSet<&'static str> = HashSet::new();
    for entry in PROVIDER_CATALOG {
        if !seen.insert(entry.source.as_str()) {
            return Err("Catalog source 重复");
        }
    }
    Ok(())
}

fn check_recommended_models() -> Result<(), &'static str> {
    // 推荐模型列表允许为空（条目无核验证据时由 Connect 阶段要求用户填写），
    // 但每条非空条目都必须有合法窗口，并保证 evidence_url 字段配套（类型系统
    // 已强制；这里再次校验为 0 → 防御）。
    for entry in PROVIDER_CATALOG {
        for model in entry.recommended_models {
            if model.context_window == 0 || model.max_tokens == 0 {
                return Err("推荐模型窗口非法");
            }
            if model.max_tokens as usize > model.context_window {
                return Err("推荐模型 max_tokens 超出 context_window");
            }
            if model.evidence_url.is_empty() {
                return Err("推荐模型 evidence_url 不得为空");
            }
        }
    }
    Ok(())
}
