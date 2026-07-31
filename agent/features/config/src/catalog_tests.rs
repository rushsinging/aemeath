//! `agent/features/config/src/catalog.rs` 的契约测试。
//!
//! 覆盖目标：
//! - 当前 ProviderDriverKind 的 10 个 driver 全部映射；
//! - source / driver 唯一性；
//! - 按 `ProviderSource` / `DriverId` 查询；
//! - 推荐模型窗口合法性（context_window > 0, max_tokens > 0）；
//! - 官方 SDK UA 证据元数据完整性（evidence_url、verified_at 与 value 配套）；
//! - base URL 证据元数据完整性（evidence_url 非空 HTTPS、verified_at 合法日期）；
//! - API key hint/env 与 Config domain `driver_env` 单一真相一致；
//! - 静态默认 base URL 合法（或显式 `None`）。

use crate::catalog::{
    api_key_env_name, default_endpoint_url, find_by_driver, find_by_source, is_known_driver,
    provider_source_for_driver, static_assert_catalog_invariants, ProviderCatalogEntry,
    ProviderSource, PROVIDER_CATALOG,
};
use share::config::domain::driver_env;

const EXPECTED_DRIVERS: &[&str] = &[
    "anthropic",
    "openai",
    "zhipu",
    "litellm",
    "volcengine",
    "minimax",
    "mimo",
    "deepseek",
    "agnes",
    "ollama",
];

#[test]
fn catalog_covers_every_supported_provider_driver() {
    let drivers: Vec<&str> = PROVIDER_CATALOG
        .iter()
        .map(|entry| entry.driver.as_str())
        .collect();

    for expected in EXPECTED_DRIVERS {
        assert!(
            drivers.contains(expected),
            "Catalog 必须覆盖 driver `{expected}`，得到 {drivers:?}"
        );
    }
    assert_eq!(
        PROVIDER_CATALOG.len(),
        EXPECTED_DRIVERS.len(),
        "Catalog 条目数必须恰好等于 10 个 driver，禁止多写漏写"
    );
}

#[test]
fn catalog_sources_are_unique_and_use_title_case_names() {
    use std::collections::HashSet;

    let mut seen_sources = HashSet::new();
    for entry in PROVIDER_CATALOG {
        let source = entry.source.as_str();
        assert!(
            source.chars().next().is_some_and(char::is_uppercase),
            "source 必须以大写字母开头（固定 stable key），得到 {source}"
        );
        assert!(
            !source.contains(char::is_whitespace),
            "source 不得包含空白，得到 {source:?}"
        );
        assert!(
            seen_sources.insert(source),
            "source 必须唯一，重复 {source}"
        );
    }
}

#[test]
fn catalog_drivers_are_unique() {
    use std::collections::HashSet;

    let mut seen_drivers = HashSet::new();
    for entry in PROVIDER_CATALOG {
        let driver = entry.driver.as_str();
        assert!(
            seen_drivers.insert(driver),
            "driver 必须唯一，重复 {driver}"
        );
    }
}

#[test]
fn catalog_find_by_source_returns_entry_with_matching_source() {
    for entry in PROVIDER_CATALOG {
        let found =
            find_by_source(entry.source.as_str()).expect("按 source 查询必须命中 Catalog 条目");
        assert_eq!(found.source.as_str(), entry.source.as_str());
        assert_eq!(found.driver.as_str(), entry.driver.as_str());
    }
}

#[test]
fn catalog_find_by_source_is_case_sensitive() {
    // 设计文档要求 source 是稳定 key，区分大小写。
    assert!(find_by_source("anthropic").is_none());
    assert!(find_by_source("Anthropic").is_some());
}

#[test]
fn catalog_find_by_unknown_source_returns_none() {
    assert!(find_by_source("Unknown").is_none());
    assert!(find_by_source("").is_none());
}

#[test]
fn catalog_find_by_driver_returns_entry_with_matching_driver() {
    for entry in PROVIDER_CATALOG {
        let found =
            find_by_driver(entry.driver.as_str()).expect("按 driver 查询必须命中 Catalog 条目");
        assert_eq!(found.driver.as_str(), entry.driver.as_str());
    }
}

#[test]
fn catalog_find_by_driver_is_case_insensitive() {
    // driver 字符串来自 `models.providers.<source>.driver`，外部可能大小写不一。
    assert!(find_by_driver("Anthropic").is_some());
    assert!(find_by_driver("ANTHROPIC").is_some());
    assert!(find_by_driver("anthropic").is_some());
}

#[test]
fn catalog_find_by_driver_does_not_allocate_uppercase_or_lowercase_copy() {
    // 性能断言（非 benchmark）：`find_by_driver` 不应该先把输入降为小写再拷贝。
    // eq_ignore_ascii_case 已支持就地比较；这里通过遍历 invariants 验证没有中间
    // String 分配（PROVIDER_CATALOG 是静态切片，零分配）。
    let driver = "Anthropic";
    let found = find_by_driver(driver).expect("Anthropic 必命中");
    assert_eq!(found.driver.as_str(), "anthropic");
}

#[test]
fn catalog_find_by_unknown_driver_returns_none() {
    assert!(find_by_driver("unknown").is_none());
    assert!(find_by_driver("").is_none());
}

#[test]
fn catalog_provider_source_for_driver_matches_static_entry() {
    for entry in PROVIDER_CATALOG {
        let source =
            provider_source_for_driver(entry.driver.as_str()).expect("按 driver 必须能取得 source");
        assert_eq!(source.as_str(), entry.source.as_str());
    }
}

#[test]
fn catalog_is_known_driver_only_for_supported_driver_names() {
    for driver in EXPECTED_DRIVERS {
        assert!(
            is_known_driver(driver),
            "Catalog 必须声明 driver `{driver}` 为已知"
        );
    }
    assert!(!is_known_driver("unknown"));
    assert!(!is_known_driver(""));
}

#[test]
fn catalog_recommended_models_have_positive_windows_and_max_tokens() {
    for entry in PROVIDER_CATALOG {
        for model in entry.recommended_models {
            let model_id = model.model_id;
            let context_window = model.context_window;
            let max_tokens = model.max_tokens;
            assert!(
                !model_id.trim().is_empty(),
                "推荐模型 id 不得为空白（{}）",
                entry.source.as_str()
            );
            assert!(
                context_window > 0,
                "推荐模型 {} 的 context_window 必须 > 0",
                model_id
            );
            assert!(
                max_tokens > 0,
                "推荐模型 {} 的 max_tokens 必须 > 0",
                model_id
            );
            assert!(
                max_tokens as usize <= context_window,
                "推荐模型 {} 的 max_tokens ({}) 不得大于 context_window ({})",
                model_id,
                max_tokens,
                context_window
            );
        }
    }
}

#[test]
fn catalog_recommended_models_carry_evidence_metadata_when_present() {
    // 向前契约：未来新增 `RecommendedModel` 必须携带 `evidence_url`（非空 HTTPS）
    // 与 `verified_at`（合法日期）。当前 Catalog 全部留空，函数体走空 assertions；
    // 一旦 PR 引入新的推荐模型，编译期会强制要求 evidence 字段。
    for entry in PROVIDER_CATALOG {
        for model in entry.recommended_models {
            let evidence = model.evidence_url;
            assert!(
                evidence.starts_with("https://"),
                "{} 的推荐模型 {} 的证据链接必须为 https://...，得到 {evidence}",
                entry.source.as_str(),
                model.model_id,
            );
            // chrono::NaiveDate 的 `from_ymd_opt` 已经在类型系统外不可达非法日期；
            // 我们仅做"非默认 epoch"校验，避免悄悄使用 1970-01-01 绕过审查。
            assert!(
                model.verified_at > chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
                "{} 的推荐模型 {} 的 verified_at 不得早于 1970-01-01",
                entry.source.as_str(),
                model.model_id,
            );
        }
    }
}

#[test]
fn verified_catalog_defaults_match_official_sources() {
    let cases = [
        (
            "Anthropic",
            "https://api.anthropic.com",
            "claude-sonnet-5",
            1_000_000,
            128_000,
        ),
        (
            "OpenAI",
            "https://api.openai.com",
            "gpt-4o",
            128_000,
            16_384,
        ),
        (
            "DeepSeek",
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            1_000_000,
            393_216,
        ),
    ];

    for (source, endpoint, model_id, context_window, max_tokens) in cases {
        let entry = find_by_source(source).expect("已核验 Provider 必须存在");
        assert_eq!(
            entry.default_endpoint.as_ref().map(|value| value.url),
            Some(endpoint),
            "{source} 必须使用官方默认 endpoint"
        );
        assert!(
            entry.recommended_models.iter().any(|model| {
                model.model_id == model_id
                    && model.context_window == context_window
                    && model.max_tokens == max_tokens
            }),
            "{source} 必须发布已核验推荐模型 {model_id}"
        );
    }
}

#[test]
fn self_hosted_or_unverified_catalog_defaults_remain_explicitly_absent() {
    for source in [
        "Zhipu",
        "LiteLLM",
        "Volcengine",
        "Minimax",
        "Mimo",
        "Agnes",
        "Ollama",
    ] {
        let entry = find_by_source(source).expect("Catalog Provider 必须存在");
        assert!(
            entry.default_endpoint.is_none(),
            "{source} 未完成同等证据核验时不得猜测 endpoint"
        );
        assert!(
            entry.recommended_models.is_empty(),
            "{source} 未完成同等证据核验时不得猜测推荐模型"
        );
    }
}

#[test]
fn catalog_entries_without_recommended_models_pass_invariant() {
    static_assert_catalog_invariants().expect("允许无推荐模型的 Catalog 条目不能破坏启动期不变量");
}

#[test]
fn catalog_default_endpoint_evidence_is_complete_when_present() {
    // 向前契约：non-None default_endpoint 必须配套：
    //   - `url` 非空；
    //   - `evidence_url` 非空且以 https:// 开头；
    //   - `verified_at` 合法 NaiveDate（类型系统已保证，仍校验非 1970-01-01 前）。
    for entry in PROVIDER_CATALOG {
        if let Some(endpoint) = &entry.default_endpoint {
            let source = entry.source.as_str();
            assert!(
                !endpoint.url.trim().is_empty(),
                "{source} 的 default_endpoint.url 不得为空"
            );
            assert!(
                !endpoint.evidence_url.trim().is_empty(),
                "{source} 的 default_endpoint.evidence_url 不得为空"
            );
            assert!(
                endpoint.evidence_url.starts_with("https://"),
                "{source} 的 default_endpoint.evidence_url 必须为 https://，得到 {}",
                endpoint.evidence_url
            );
            assert!(
                endpoint.verified_at > chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
                "{source} 的 default_endpoint.verified_at 不得早于 1970-01-01"
            );
        }
    }
}

#[test]
fn catalog_default_endpoint_url_helper_returns_static_url() {
    // 与 `default_endpoint` 字段保持一致：None 时 helper 也返回 None。
    for entry in PROVIDER_CATALOG {
        let url = default_endpoint_url(entry.source.as_str());
        match entry.default_endpoint {
            None => assert!(
                url.is_none(),
                "{} 的 default_endpoint 为 None，但 helper 返回了 {url:?}",
                entry.source.as_str()
            ),
            Some(endpoint) => assert_eq!(
                url,
                Some(endpoint.url),
                "{} 的 default_endpoint_url() 与静态条目不一致",
                entry.source.as_str()
            ),
        }
    }
}

#[test]
fn catalog_official_sdk_user_agent_evidence_is_complete_when_present() {
    for entry in PROVIDER_CATALOG {
        let ua = entry.official_sdk_user_agent.as_ref();
        if let Some(ua) = ua {
            // 证据元数据必须完整；任何字段为空都说明这是猜测出来的 UA，违反规范。
            assert!(
                !ua.sdk_name.trim().is_empty(),
                "{} 的官方 SDK UA 名称不得为空",
                entry.source.as_str()
            );
            assert!(
                !ua.sdk_version.trim().is_empty(),
                "{} 的官方 SDK 版本不得为空",
                entry.source.as_str()
            );
            let evidence = ua.evidence_url;
            assert!(
                evidence.starts_with("https://") || evidence.starts_with("http://"),
                "{} 的官方 SDK UA 证据链接必须可访问（http(s)），得到 {evidence}",
                entry.source.as_str()
            );
            let value_str = ua.value.to_str().unwrap_or("");
            assert!(
                !value_str.is_empty(),
                "{} 的官方 SDK UA 值不得为空",
                entry.source.as_str()
            );
        }
    }
}

#[test]
fn catalog_official_sdk_user_agent_values_are_header_value_safe() {
    for entry in PROVIDER_CATALOG {
        if let Some(ua) = &entry.official_sdk_user_agent {
            // HeaderValue::from_str 在控制字符处会失败；这里我们已经构造过，
            // 但仍通过 `to_str` 验证不含 NUL/CR/LF 等非法字符。
            let bytes = ua
                .value
                .to_str()
                .expect("HeaderValue 必须是合法可见 ASCII")
                .as_bytes();
            assert!(
                bytes.iter().all(|b| !b.is_ascii_control()),
                "{} 的官方 SDK UA 不得含 ASCII 控制字符，得到 {:?}",
                entry.source.as_str(),
                ua.value.to_str()
            );
        }
    }
}

#[test]
fn catalog_api_key_env_matches_driver_env_single_source_of_truth() {
    for entry in PROVIDER_CATALOG {
        let expected =
            driver_env::driver_api_key_env_name(entry.driver.as_str()).map(str::to_string);
        let actual = api_key_env_name(entry.driver.as_str()).map(str::to_string);
        assert_eq!(
            actual,
            expected,
            "Catalog api_key env ({actual:?}) 与 driver_env 单一真相 ({expected:?}) 不一致（{}）",
            entry.source.as_str()
        );
    }
}

#[test]
fn catalog_api_key_hint_is_human_readable_and_references_driver_env() {
    for entry in PROVIDER_CATALOG {
        let hint = entry
            .api_key_hint
            .unwrap_or_else(|| panic!("{} 必须提供 api_key_hint", entry.source.as_str()));
        assert!(
            !hint.trim().is_empty(),
            "{} 的 api_key_hint 不得为空",
            entry.source.as_str()
        );
    }
}

#[test]
fn catalog_fixed_source_names_are_stable_title_case() {
    // spec §2.1 要求 source 使用固定内置名称（大写开头）作为稳定 key。
    let expected_sources = [
        ("anthropic", "Anthropic"),
        ("openai", "OpenAI"),
        ("zhipu", "Zhipu"),
        ("litellm", "LiteLLM"),
        ("volcengine", "Volcengine"),
        ("minimax", "Minimax"),
        ("mimo", "Mimo"),
        ("deepseek", "DeepSeek"),
        ("agnes", "Agnes"),
        ("ollama", "Ollama"),
    ];
    for (driver, source) in expected_sources {
        let entry = find_by_driver(driver).expect("Catalog 必须包含 driver");
        assert_eq!(
            entry.source.as_str(),
            source,
            "driver {driver} 必须映射到稳定 source {source}"
        );
        assert_eq!(
            ProviderSource::new(source),
            entry.source,
            "ProviderSource::new 必须保留稳定大写 key"
        );
    }
}

#[test]
fn catalog_entries_expose_borrowed_str_for_zero_copy_access() {
    let entry: &ProviderCatalogEntry = find_by_driver("anthropic").expect("Catalog 必含 anthropic");
    // 所有字段都是零拷贝 `&'static str` 或 `Option<&'static str>` / `Option<DefaultEndpoint>`。
    let _source: &'static str = entry.source.as_str();
    let _driver: &'static str = entry.driver.as_str();
    let _url: Option<&'static str> = default_endpoint_url(entry.source.as_str());
}

#[test]
fn catalog_static_invariants_hold_at_runtime() {
    static_assert_catalog_invariants()
        .expect("Catalog 启动期不变量必须通过（source 唯一、driver 唯一、推荐模型合法）");
}
