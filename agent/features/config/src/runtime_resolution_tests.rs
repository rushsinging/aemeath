//! Config-owned Provider Runtime Resolution 的失败契约测试。

use super::{ProviderRuntimeResolver, ResolvedProviderRuntimeConfig};
use crate::ports::SystemInformation;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::models::{ModelEntryConfig, ProviderModelsConfig};
use share::config::Config;

fn resolver() -> ProviderRuntimeResolver {
    ProviderRuntimeResolver::new(
        SystemInformation {
            os_name: "linux".to_string(),
            os_version: Some("6.1.0".to_string()),
            arch: "x86_64".to_string(),
        },
        "0.1.0",
    )
}

fn snapshot(
    source: &str,
    driver: &str,
    base_url: &str,
    user_agent: Option<&str>,
) -> ConfigSnapshot {
    snapshot_with_global_user_agent(source, driver, base_url, user_agent, "global/1.0")
}

fn snapshot_with_global_user_agent(
    source: &str,
    driver: &str,
    base_url: &str,
    user_agent: Option<&str>,
    global_user_agent: &str,
) -> ConfigSnapshot {
    let mut config = Config::default();
    config.api.user_agent = global_user_agent.to_string();
    config.models.default = format!("{source}/model");
    config.models.providers.insert(
        source.to_string(),
        ProviderModelsConfig {
            driver: driver.to_string(),
            base_url: base_url.to_string(),
            user_agent: user_agent.map(str::to_string),
            api_key: "secret".to_string(),
            models: vec![ModelEntryConfig {
                id: "model".to_string(),
                context_window: 128_000,
                max_tokens: 8_192,
                name: String::new(),
                input: Vec::new(),
                reasoning: None,
                reasoning_effort: None,
                api_style: None,
            }],
        },
    );
    ConfigSnapshot::new(config)
}

fn resolve(
    snapshot: &ConfigSnapshot,
    source: &str,
    driver: &str,
    base_url_override: Option<&str>,
) -> ResolvedProviderRuntimeConfig {
    let resolved = snapshot
        .resolve_model_selection(&format!("{source}/model"))
        .unwrap();
    assert_eq!(resolved.driver, driver);
    resolver().resolve(snapshot, &resolved, base_url_override)
}

#[test]
fn runtime_resolution_uses_catalog_endpoint_when_provider_endpoint_is_empty() {
    let snapshot = snapshot("Anthropic", "anthropic", "", None);

    let resolved = resolve(&snapshot, "Anthropic", "anthropic", None);

    assert_eq!(
        resolved.base_url.as_deref(),
        Some("https://api.anthropic.com")
    );
}

#[test]
fn runtime_resolution_uses_source_specific_endpoint_for_shared_driver() {
    for (source, expected_endpoint) in [
        ("Zhipu", "https://open.bigmodel.cn/api/paas/v4"),
        (
            "Zhipu Coding Plan",
            "https://open.bigmodel.cn/api/coding/paas/v4",
        ),
    ] {
        let snapshot = snapshot(source, "zhipu", "", None);
        let resolved = resolve(&snapshot, source, "zhipu", None);
        assert_eq!(resolved.base_url.as_deref(), Some(expected_endpoint));
    }
}

#[test]
fn runtime_resolution_uses_explicit_endpoint_override_before_catalog() {
    let snapshot = snapshot("Anthropic", "anthropic", "", None);

    let resolved = resolve(
        &snapshot,
        "Anthropic",
        "anthropic",
        Some("https://proxy.example.test"),
    );

    assert_eq!(
        resolved.base_url.as_deref(),
        Some("https://proxy.example.test")
    );
}

#[test]
fn runtime_resolution_prefers_global_user_agent_over_catalog_official_sdk() {
    // 全局 `api.user_agent` 是用户显式表达的身份意图，优先于 Catalog 自动 UA
    // （Anthropic 已核验官方客户端 UA 也不得压过显式全局配置）。
    let snapshot = snapshot("Anthropic", "anthropic", "", None);

    let resolved = resolve(&snapshot, "Anthropic", "anthropic", None);

    assert_eq!(resolved.user_agent, "global/1.0");
}

#[test]
fn runtime_resolution_falls_back_to_catalog_official_sdk_when_global_is_blank() {
    // 全局 UA 空白等同未配置；此时 Anthropic（已核验官方客户端 UA）的
    // Catalog 级必须兜底生效。
    let snapshot = snapshot_with_global_user_agent("Anthropic", "anthropic", "", None, "   ");

    let resolved = resolve(&snapshot, "Anthropic", "anthropic", None);

    assert_eq!(
        resolved.user_agent,
        "claude-cli/2.1.267 (external, sdk-cli)"
    );
}

#[test]
fn runtime_resolution_skips_catalog_level_without_sdk_evidence() {
    // DeepSeek 无官方 SDK UA；Catalog 级保持 None 并回退到全局配置。
    let snapshot = snapshot("DeepSeek", "deepseek", "", None);

    let resolved = resolve(&snapshot, "DeepSeek", "deepseek", None);

    assert_eq!(resolved.user_agent, "global/1.0");
}

#[test]
fn runtime_resolution_applies_provider_user_agent_before_global_user_agent() {
    let snapshot = snapshot("Anthropic", "anthropic", "", Some("provider/1.0"));

    let resolved = resolve(&snapshot, "Anthropic", "anthropic", None);

    assert_eq!(resolved.user_agent, "provider/1.0");
}

#[test]
fn runtime_resolution_falls_back_to_global_user_agent() {
    let snapshot = snapshot(
        "DeepSeek",
        "deepseek",
        "https://deepseek.example.test",
        None,
    );

    let resolved = resolve(&snapshot, "DeepSeek", "deepseek", None);

    assert_eq!(resolved.user_agent, "global/1.0");
}

#[test]
fn runtime_resolution_falls_back_to_global_default_user_agent() {
    let mut snapshot = snapshot(
        "DeepSeek",
        "deepseek",
        "https://deepseek.example.test",
        None,
    );
    let mut config = snapshot.to_config();
    config.api.user_agent.clear();
    snapshot = ConfigSnapshot::new(config);

    let resolved = resolve(&snapshot, "DeepSeek", "deepseek", None);

    assert_eq!(resolved.user_agent, "Aemeath/0.1.0 cli linux/6.1.0/x86_64");
}
