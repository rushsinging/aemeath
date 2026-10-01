//! Config domain 静态数据常量（#1146 归位：自 `crate::catalog` 行为文件抽出）。
//!
//! Catalog 是 Config domain 的静态数据集（见 `catalog.rs` 模块文档）：Provider 条目、
//! 推荐模型、证据 URL 与核验日期统一归 domain 层 constants；`catalog.rs` 经
//! `pub use` 保持 `config::catalog::PROVIDER_CATALOG` 公共路径不变。

use crate::catalog::{
    DefaultEndpoint, DriverId, OfficialSdkUserAgent, ProviderCatalogEntry, ProviderSource,
    RecommendedModel,
};

// ==========================================================================
// ---------------------------------------------------------------------------
// 静态 Catalog：覆盖 Connect 暴露的 9 个 runtime driver（10 个内置 source）；
// 同一 driver 可对应多个稳定 source（例如普通 Zhipu 与 Zhipu Coding Plan）。
//
// Volcengine driver 仍在 Provider crate 中受支持（`VOLCENGINE_CODING_PLAN_API_KEY`
// 环境变量与协议 ACL 不变），但其 endpoint 与推荐模型都没有可核验证据，按
// 「无证据即留空」原则不进入 Catalog，因此不出现在 Connect 向导中。
//
// 所有条目字段都是 `&'static str` / `Option<&'static str>` / `&'static [..]`，零拷贝。
// 构建期由 `static_assert_catalog_invariants` 验证：
//   - source 唯一；
//   - 推荐模型窗口合法（context_window > 0, max_tokens in (0, context_window]）；
//   - 官方 SDK UA 字段配套完整、HeaderValue 可解析。
//
// 非空条目必须配套 evidence 元数据；不可核验条目继续保持为空。
// ---------------------------------------------------------------------------

/// endpoint 与证据元数据在 2026-08-05 核验。
const VERIFIED_AT: chrono::NaiveDate = chrono::NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();

/// 推荐模型在 2026-09-10 按各家官方模型文档重新核验。
const VERIFIED_AT_2026_09_10: chrono::NaiveDate =
    chrono::NaiveDate::from_ymd_opt(2026, 9, 10).unwrap();

const OPENAI_MODEL_EVIDENCE: &str = "https://developers.openai.com/api/docs/models";
const ANTHROPIC_MODEL_EVIDENCE: &str =
    "https://platform.claude.com/docs/en/about-claude/models/overview.md";
const MINIMAX_MODEL_EVIDENCE: &str =
    "https://platform.minimaxi.com/docs/api-reference/text-openai-api";
const DEEPSEEK_MODEL_EVIDENCE: &str = "https://api-docs.deepseek.com/quick_start/pricing";

const OPENAI_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        model_id: "gpt-6-astra",
        context_window: 1_050_000,
        max_tokens: 128_000,
        evidence_url: OPENAI_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "gpt-5.6-sol",
        context_window: 1_050_000,
        max_tokens: 128_000,
        evidence_url: OPENAI_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT,
    },
    RecommendedModel {
        model_id: "gpt-5.6-terra",
        context_window: 1_050_000,
        max_tokens: 128_000,
        evidence_url: OPENAI_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT,
    },
    RecommendedModel {
        model_id: "gpt-5.6-luna",
        context_window: 1_050_000,
        max_tokens: 128_000,
        evidence_url: OPENAI_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT,
    },
];

/// Anthropic 当前在线模型。
///
/// 官方文档公布 4.6 世代起 dateless 模型 ID 即固定快照（非浮动指针），因此直接
/// 使用 dateless ID；`claude-haiku-4-5` 是官方列出的 Claude API alias。
const ANTHROPIC_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        model_id: "claude-fable-5-1",
        context_window: 1_000_000,
        max_tokens: 131_072,
        evidence_url: ANTHROPIC_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "claude-opus-5",
        context_window: 1_000_000,
        max_tokens: 131_072,
        evidence_url: ANTHROPIC_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "claude-sonnet-5",
        context_window: 1_000_000,
        max_tokens: 131_072,
        evidence_url: ANTHROPIC_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "claude-haiku-4-5",
        context_window: 200_000,
        max_tokens: 64_000,
        evidence_url: ANTHROPIC_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
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
    // 本地抓包核验：Claude Code CLI 2.1.267 向 `/v1/messages` 发送的
    // `User-Agent` 逐字符为 `claude-cli/2.1.267 (external, sdk-cli)`。
    official_sdk_user_agent: Some(OfficialSdkUserAgent {
        sdk_name: "claude-code-cli",
        sdk_version: "2.1.267",
        value: "claude-cli/2.1.267 (external, sdk-cli)",
        evidence_url: "https://github.com/anthropics/claude-code",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
};

const ZHIPU_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        model_id: "glm-5.3",
        context_window: 1_000_000,
        max_tokens: 131_072,
        evidence_url: "https://docs.bigmodel.cn/cn/guide/models/text/glm-5.3",
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        // GLM-5.3-Flash/FlashX：首个原生多模态（视觉 Coding），1M 上下文
        // 与 128K 最大输出（官方模型页核验）。
        model_id: "glm-5.3-flash",
        context_window: 1_048_576,
        max_tokens: 131_072,
        evidence_url: "https://docs.bigmodel.cn/cn/guide/models/vlm/glm-5.3-flash",
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "glm-5.2",
        context_window: 1_000_000,
        max_tokens: 131_072,
        evidence_url: "https://docs.bigmodel.cn/cn/guide/models/text/glm-5.2",
        verified_at: VERIFIED_AT,
    },
    RecommendedModel {
        model_id: "glm-5-turbo",
        context_window: 200_000,
        max_tokens: 131_072,
        evidence_url: "https://docs.bigmodel.cn/cn/guide/models/text/glm-5-turbo",
        verified_at: VERIFIED_AT,
    },
];

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
    // 本地抓包核验：codex CLI 0.154.0 的 `POST /v1/responses` 发送的 UA 逐字符为
    // `codex_exec/0.154.0 (Mac OS 26.2.0; arm64) ghostty/1.3.2-HEAD-_bb30526 (codex_exec; 0.154.0)`。
    //
    // 已知限制：该字符串包含抓包环境的操作系统版本、架构与终端标识，三者都是运行
    // 时动态段。当前按产品决策固化抓包值，只在同类环境下逐字符吻合；换 OS / 架构 /
    // 终端后需要重新核验或改为模板化表达（见设计文档 §5.1.1）。
    official_sdk_user_agent: Some(OfficialSdkUserAgent {
        sdk_name: "codex-cli",
        sdk_version: "0.154.0",
        value: "codex_exec/0.154.0 (Mac OS 26.2.0; arm64) ghostty/1.3.2-HEAD-_bb30526 (codex_exec; 0.154.0)",
        evidence_url: "https://github.com/openai/codex",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
};

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
    // ZCode 是 BigModel（国内）与 Z.ai（海外）双平台的官方桌面客户端；其配置文档
    // 的端点矩阵覆盖通用余额端点（资源包 / 充值余额场景），因此普通开放平台
    // 请求同样携带 ZCode 客户端标识。GLM 请求头构造为
    // `User-Agent: ZCode/${appVersion}`（伴随 `X-ZCode-Agent` 与
    // `X-ZCode-App-Version`）；appVersion 取自应用 Info.plist 的
    // CFBundleShortVersionString = 3.11.2，因此 UA 逐字符为 `ZCode/3.11.2`。
    official_sdk_user_agent: Some(OfficialSdkUserAgent {
        sdk_name: "zcode",
        sdk_version: "3.11.2",
        value: "ZCode/3.11.2",
        evidence_url: "https://zcode.z.ai/cn/docs/configuration",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
};

/// Zhipu Coding Plan 使用相同 runtime driver，但拥有独立稳定 source 与 endpoint。
const ZHIPU_CODING_PLAN_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Zhipu Coding Plan"),
    driver: DriverId::new("zhipu"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://open.bigmodel.cn/api/coding/paas/v4",
        evidence_url: "https://docs.bigmodel.cn/cn/coding-plan/third-party-integration",
        verified_at: VERIFIED_AT,
    }),
    recommended_models: ZHIPU_MODELS,
    api_key_hint: Some("智谱 Coding Plan → API Keys"),
    // ZCode 3.11.2 是智谱 Coding Plan 的官方桌面客户端。其 `glm/zcode.cjs` 的
    // GLM 请求头构造为 `User-Agent: ZCode/${appVersion}`（伴随
    // `X-ZCode-Agent: glm` 与 `X-ZCode-App-Version`）；appVersion 取自应用
    // Info.plist 的 CFBundleShortVersionString = 3.11.2，因此 UA 逐字符为
    // `ZCode/3.11.2`。
    official_sdk_user_agent: Some(OfficialSdkUserAgent {
        sdk_name: "zcode",
        sdk_version: "3.11.2",
        value: "ZCode/3.11.2",
        evidence_url: "https://zcode.z.ai/cn/docs/configuration",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
};

/// Z.ai 是智谱面向海外用户的国际平台（美元计价），与 BigModel 同模型体系但
/// 账号、余额与 API Key 互不通用。通用端点服务资源包 / 充值余额场景。
const ZAI_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Z.ai"),
    driver: DriverId::new("zhipu"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://api.z.ai/api/paas/v4",
        evidence_url: "https://zcode.z.ai/cn/docs/configuration",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
    recommended_models: ZHIPU_MODELS,
    api_key_hint: Some("Z.ai Platform → API Keys"),
    // ZCode 同为 BigModel 与 Z.ai 的官方桌面客户端（ZCode 配置文档端点矩阵
    // 列明 `https://api.z.ai/api/paas/v4` 通用端点），UA 与国内系列一致。
    official_sdk_user_agent: Some(OfficialSdkUserAgent {
        sdk_name: "zcode",
        sdk_version: "3.11.2",
        value: "ZCode/3.11.2",
        evidence_url: "https://zcode.z.ai/cn/docs/configuration",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
};

/// Z.ai Coding Plan 使用 Coding 专用端点（不可与通用端点互相替代、额度独立）。
const ZAI_CODING_PLAN_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Z.ai Coding Plan"),
    driver: DriverId::new("zhipu"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://api.z.ai/api/coding/paas/v4",
        evidence_url: "https://zcode.z.ai/cn/docs/configuration",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
    recommended_models: ZHIPU_MODELS,
    api_key_hint: Some("Z.ai Coding Plan → API Keys"),
    official_sdk_user_agent: Some(OfficialSdkUserAgent {
        sdk_name: "zcode",
        sdk_version: "3.11.2",
        value: "ZCode/3.11.2",
        evidence_url: "https://zcode.z.ai/cn/docs/configuration",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
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

/// MiniMax (MiniMax) Catalog 条目。
///
/// 官方 OpenAI 兼容文档公布在线模型与上下文窗口，但未公布最大输出 token；
/// `max_tokens` 统一采用 131_072（128K）是经确认的产品决策豁免，理由与风险
/// 记录在 `docs/design/02-modules/config/02-provider-catalog-and-connect.md`。
const MINIMAX_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        model_id: "MiniMax-M3",
        context_window: 1_000_000,
        max_tokens: 131_072,
        evidence_url: MINIMAX_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "MiniMax-M2.7",
        context_window: 204_800,
        max_tokens: 131_072,
        evidence_url: MINIMAX_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "MiniMax-M2.5",
        context_window: 204_800,
        max_tokens: 131_072,
        evidence_url: MINIMAX_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "MiniMax-M2.1",
        context_window: 204_800,
        max_tokens: 131_072,
        evidence_url: MINIMAX_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
];

const MINIMAX_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Minimax"),
    driver: DriverId::new("minimax"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://api.minimaxi.com/v1",
        evidence_url: "https://platform.minimaxi.com/docs/api-reference/text-openai-api",
        verified_at: VERIFIED_AT,
    }),
    recommended_models: MINIMAX_MODELS,
    api_key_hint: Some("Minimax 开放平台 → API Keys"),
    official_sdk_user_agent: None,
};

/// Minimax.io 是 MiniMax 面向海外用户的国际平台（美元计价），与国内平台
/// 同模型体系但账号、余额与 API Key 互不通用。
const MINIMAX_INTERNATIONAL_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Minimax.io"),
    driver: DriverId::new("minimax"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://api.minimax.io/v1",
        evidence_url: "https://platform.minimax.io/docs/api-reference/text-openai-api",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
    recommended_models: MINIMAX_MODELS,
    api_key_hint: Some("MiniMax Platform (International) → API Keys"),
    official_sdk_user_agent: None,
};

/// Xiaomi MiMo Catalog 条目。
///
/// `mimo-v2-pro` / `mimo-v2-flash` 等旧模型已于 2026-06-30 下线，
/// 只收录当前在线的 V2.5 系列。
const MIMO_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        model_id: "mimo-v2.5-pro",
        context_window: 1_000_000,
        max_tokens: 131_072,
        evidence_url: "https://mimo.mi.com/docs/zh-CN/quick-start/summary/model",
        verified_at: VERIFIED_AT,
    },
    RecommendedModel {
        model_id: "mimo-v2.5",
        context_window: 1_000_000,
        max_tokens: 131_072,
        evidence_url: "https://mimo.mi.com/docs/zh-CN/quick-start/summary/model",
        verified_at: VERIFIED_AT,
    },
];

const MIMO_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Mimo"),
    driver: DriverId::new("mimo"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://api.xiaomimimo.com/v1",
        evidence_url: "https://mimo.mi.com/docs/zh-CN/quick-start/summary/first-api-call",
        verified_at: VERIFIED_AT,
    }),
    recommended_models: MIMO_MODELS,
    api_key_hint: Some("Xiaomi MiMo 开放平台 → API Keys"),
    official_sdk_user_agent: None,
};

/// MiMo Token Plan：固定订阅费套餐使用专属 BASE_URL 与专属 API Key，
/// 与按量付费端点互不通用（官方文档"如使用 Token Plan 需替换 BASE_URL"）。
const MIMO_TOKEN_PLAN_ENTRY: ProviderCatalogEntry = ProviderCatalogEntry {
    source: ProviderSource::new("Mimo Token Plan"),
    driver: DriverId::new("mimo"),
    default_endpoint: Some(DefaultEndpoint {
        url: "https://token-plan-cn.xiaomimimo.com/v1",
        evidence_url: "https://mimo.mi.com/docs/zh-CN/quick-start/summary/first-api-call",
        verified_at: VERIFIED_AT_2026_09_10,
    }),
    recommended_models: MIMO_MODELS,
    api_key_hint: Some("Xiaomi MiMo 开放平台 → Token Plan 专属 API Key"),
    official_sdk_user_agent: None,
};

/// DeepSeek 当前在线模型。
///
/// 2026-09-10 核验：`deepseek-v4-flash` 已退役，官方模型名改为 `deepseek-flash`
/// （DeepSeek-V4.1-Flash 承接其请求）；`deepseek-v4-pro` 自 2026-09-14 起同样路由
/// 到 V4.1-Flash。
const DEEPSEEK_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        model_id: "deepseek-v4-pro",
        context_window: 1_000_000,
        max_tokens: 393_216,
        evidence_url: DEEPSEEK_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
    RecommendedModel {
        model_id: "deepseek-flash",
        context_window: 1_000_000,
        max_tokens: 393_216,
        evidence_url: DEEPSEEK_MODEL_EVIDENCE,
        verified_at: VERIFIED_AT_2026_09_10,
    },
];

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
/// 另一份默认值。所有查询通过 [`crate::catalog::find_by_source`] / [`crate::catalog::find_by_driver`] 完成。
pub static PROVIDER_CATALOG: &[ProviderCatalogEntry] = &[
    ANTHROPIC_ENTRY,
    OPENAI_ENTRY,
    ZHIPU_ENTRY,
    ZHIPU_CODING_PLAN_ENTRY,
    ZAI_ENTRY,
    ZAI_CODING_PLAN_ENTRY,
    LITELLM_ENTRY,
    MINIMAX_ENTRY,
    MINIMAX_INTERNATIONAL_ENTRY,
    MIMO_ENTRY,
    MIMO_TOKEN_PLAN_ENTRY,
    DEEPSEEK_ENTRY,
    AGNES_ENTRY,
    OLLAMA_ENTRY,
];

// ==========================================================================
// 启动期不变量校验伴生静态（`crate::catalog::static_assert_catalog_invariants`）
// ==========================================================================
// `Result<(), &'static str>` 本身不是 `Copy`（discriminant + 变体大小异质），
// 但内部字符串字面量是 `'static`，因此这里只需在首次失败时把消息搬到外面。
pub(crate) static VALIDATION_CACHE: std::sync::OnceLock<Result<(), &'static str>> =
    std::sync::OnceLock::new();
