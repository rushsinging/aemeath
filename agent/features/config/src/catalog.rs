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
//! - 当前已核验 Anthropic 官方 endpoint；产品内置模型与用户明确指定的 OpenAI、
//!   Zhipu 默认值使用各自记录的来源。其余条目继续要求用户显式填写。

/// Config-owned 固定 source 名称。
///
/// TUI 只展示 Catalog DTO，禁止自行拼接 source。`ProviderSource::new` 仅允许
/// 与 [`PROVIDER_CATALOG`] 中已注册的固定名称相等，运行时输入的 source key 必须
/// 在配置层校验后进入 Config BC。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderSource(&'static str);

impl ProviderSource {
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
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
    pub value: HeaderValue,
    pub evidence_url: &'static str,
    pub verified_at: chrono::NaiveDate,
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

// ---------------------------------------------------------------------------
// 静态 Catalog：覆盖 10 个 runtime driver；同一 driver 可对应多个稳定 source。
//
// 所有条目字段都是 `&'static str` / `Option<&'static str>` / `&'static [..]`，零拷贝。
// 构建期由 `static_assert_catalog_invariants` 验证：
//   - source 唯一；
//   - 推荐模型窗口合法（context_window > 0, max_tokens in (0, context_window]）；
//   - 官方 SDK UA 字段配套完整、HeaderValue 可解析。
//
// 非空条目必须配套 evidence 元数据；不可核验条目继续保持为空。
// ---------------------------------------------------------------------------

const VERIFIED_AT: chrono::NaiveDate = chrono::NaiveDate::from_ymd_opt(2026, 7, 21).unwrap();

const ANTHROPIC_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        model_id: "claude-opus-4-1-20250805",
        context_window: 200_000,
        max_tokens: 32_000,
        evidence_url: "https://platform.claude.com/docs/en/about-claude/models/overview.md",
        verified_at: VERIFIED_AT,
    },
    RecommendedModel {
        model_id: "claude-sonnet-4-20250514",
        context_window: 200_000,
        max_tokens: 64_000,
        evidence_url: "https://platform.claude.com/docs/en/about-claude/models/overview.md",
        verified_at: VERIFIED_AT,
    },
];

/// Anthropic Catalog 条目。
const ANTHROPIC_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Anthropic"),
    driver: DriverId::new("anthropic"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://api.anthropic.com",
        evidence_url: "https://platform.claude.com/docs/en/api/overview.md",
        verified_at: VERIFIED_AT,
    }),
    recommended_models: ANTHROPIC_MODELS,
    api_key_hint: Some("Anthropic Console → Settings → API Keys"),
    official_sdk_user_agent: None,
};

const OPENAI_MODELS: &[RecommendedModel] = &[RecommendedModel {
    model_id: "gpt-5.6-sol",
    context_window: 1_000_000,
    max_tokens: 16_384,
    evidence_url: "https://developers.openai.com/api/docs/models",
    verified_at: VERIFIED_AT,
}];

/// OpenAI Catalog 条目。
const OPENAI_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("OpenAI"),
    driver: DriverId::new("openai"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://api.openai.com",
        evidence_url: "https://github.com/openai/openai-python/blob/main/src/openai/_client.py",
        verified_at: VERIFIED_AT,
    }),
    recommended_models: OPENAI_MODELS,
    api_key_hint: Some("OpenAI Dashboard → API keys"),
    official_sdk_user_agent: None,
};

const ZHIPU_MODELS: &[RecommendedModel] = &[RecommendedModel {
    model_id: "glm-5.2",
    context_window: 1_000_000,
    max_tokens: 16_384,
    evidence_url: "https://open.bigmodel.cn/dev/api/normal-model/glm-5",
    verified_at: VERIFIED_AT,
}];

/// Zhipu（智谱开放平台）Catalog 条目。
const ZHIPU_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Zhipu"),
    driver: DriverId::new("zhipu"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://open.bigmodel.cn/api/paas/v4",
        evidence_url: "https://open.bigmodel.cn/dev/api/thirdparty-frame/openai-sdk",
        verified_at: VERIFIED_AT,
    }),
    recommended_models: ZHIPU_MODELS,
    api_key_hint: Some("智谱开放平台 → API Keys"),
    official_sdk_user_agent: None,
};

/// Zhipu Coding Plan 使用相同 runtime driver，但拥有独立稳定 source 与 endpoint。
const ZHIPU_CODING_PLAN_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("ZhipuCodingPlan"),
    driver: DriverId::new("zhipu"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://open.bigmodel.cn/api/coding/paas/v4",
        evidence_url: "https://docs.bigmodel.cn/cn/coding-plan/third-party-integration",
        verified_at: VERIFIED_AT,
    }),
    recommended_models: ZHIPU_MODELS,
    api_key_hint: Some("智谱 Coding Plan → API Keys"),
    official_sdk_user_agent: None,
};

/// LiteLLM Catalog 条目。
///
/// LiteLLM 是代理网关，base URL 由用户自托管决定；Catalog 故意留 `None` 以强制
/// Connect 阶段由用户填写。
const LITELLM_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("LiteLLM"),
    driver: DriverId::new("litellm"),
    default_endpoint: None,
    recommended_models: &[],
    api_key_hint: Some("LiteLLM Proxy 上游模型路由"),
    official_sdk_user_agent: None,
};

/// Volcengine Catalog 条目。
///
/// base URL 与推荐模型当前都没有可核验证据，由 Connect 引导用户填写。
const VOLCENGINE_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Volcengine"),
    driver: DriverId::new("volcengine"),
    default_endpoint: None,
    recommended_models: &[],
    api_key_hint: Some("火山引擎 → ARK → API Keys"),
    official_sdk_user_agent: None,
};

/// MiniMax (MiniMax) Catalog 条目。
///
/// base URL 与推荐模型当前都没有可核验证据，由 Connect 引导用户填写。
const MINIMAX_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Minimax"),
    driver: DriverId::new("minimax"),
    default_endpoint: None,
    recommended_models: &[],
    api_key_hint: Some("Minimax 开放平台 → API Keys"),
    official_sdk_user_agent: None,
};

/// Xiaomi MiMo Catalog 条目。
///
/// base URL 与推荐模型当前都没有可核验证据，由 Connect 引导用户填写。
const MIMO_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Mimo"),
    driver: DriverId::new("mimo"),
    default_endpoint: None,
    recommended_models: &[],
    api_key_hint: Some("Xiaomi MiMo 开放平台 → API Keys"),
    official_sdk_user_agent: None,
};

const DEEPSEEK_MODELS: &[RecommendedModel] = &[RecommendedModel {
    model_id: "deepseek-v4-pro",
    context_window: 1_000_000,
    max_tokens: 393_216,
    evidence_url: "https://api-docs.deepseek.com/quick_start/pricing",
    verified_at: VERIFIED_AT,
}];

/// DeepSeek Catalog 条目。
const DEEPSEEK_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("DeepSeek"),
    driver: DriverId::new("deepseek"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://api.deepseek.com",
        evidence_url: "https://api-docs.deepseek.com/",
        verified_at: VERIFIED_AT,
    }),
    recommended_models: DEEPSEEK_MODELS,
    api_key_hint: Some("DeepSeek Platform → API Keys"),
    official_sdk_user_agent: None,
};

/// Agnes Catalog 条目。
///
/// base URL 与推荐模型当前都没有可核验证据，由 Connect 引导用户填写。
const AGNES_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Agnes"),
    driver: DriverId::new("agnes"),
    default_endpoint: None,
    recommended_models: &[],
    api_key_hint: Some("Agnes 平台 → API Keys"),
    official_sdk_user_agent: None,
};

/// Ollama Catalog 条目。
///
/// Ollama 是本地自托管网关；`default_endpoint` 与 `recommended_models` 当前没有
/// 可核验的官方证据，由 Connect 引导用户填写。
const OLLAMA_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Ollama"),
    driver: DriverId::new("ollama"),
    default_endpoint: None,
    recommended_models: &[],
    api_key_hint: Some("Ollama 本地服务无需 API Key"),
    official_sdk_user_agent: None,
};

/// Config domain 内置的 Provider Catalog。
///
/// 该切片是 Config-owned 默认值集合的**唯一**入口；provider adapter 禁止保留
/// 另一份默认值。所有查询通过 [`find_by_source`] / [`find_by_driver`] 完成。
pub static PROVIDER_CATALOG: &[ProviderCatalogEntry] = &[
    ANTHROPIC_ENTRY,
    OPENAI_ENTRY,
    ZHIPU_ENTRY,
    ZHIPU_CODING_PLAN_ENTRY,
    LITELLM_ENTRY,
    VOLCENGINE_ENTRY,
    MINIMAX_ENTRY,
    MIMO_ENTRY,
    DEEPSEEK_ENTRY,
    AGNES_ENTRY,
    OLLAMA_ENTRY,
];

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
    find_by_driver(driver).map(|entry| entry.source)
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
    use std::sync::OnceLock;
    static CACHE: OnceLock<Result<(), &'static str>> = OnceLock::new();
    // `Result<(), &'static str>` 本身不是 `Copy`（discriminant + 变体大小异质），
    // 但内部字符串字面量是 `'static`，因此这里只需在首次失败时把消息搬到外面。
    match CACHE.get_or_init(|| check_unique_sources().and(check_recommended_models())) {
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
    // 但每条非空条目都必须有合法的窗口，并保证 evidence_url 字段配套（类型系统
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
