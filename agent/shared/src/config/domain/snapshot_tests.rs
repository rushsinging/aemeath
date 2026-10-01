use super::*;
use crate::config::models::ProviderModelsConfig;
use crate::config::Config;

#[test]
fn user_agent_accessor_returns_configured_value() {
    let mut config = Config::default();
    config.api.user_agent = "aemeath-test/1.0".to_string();

    assert_eq!(ConfigSnapshot::new(config).user_agent(), "aemeath-test/1.0");
}

#[test]
fn logging_accessors_publish_complete_static_settings() {
    let mut config = Config::default();
    config.logging.level = "debug".to_string();
    config.logging.logs_dir = Some("custom/logs".to_string());
    config.logging.max_bytes = 42;
    config.logging.max_backups = 3;
    config.logging.retention_days = 14;
    let snapshot = ConfigSnapshot::new(config);

    assert_eq!(snapshot.logging_level(), "debug");
    assert_eq!(snapshot.logs_dir(), Some("custom/logs"));
    assert_eq!(snapshot.logging_max_bytes(), 42);
    assert_eq!(snapshot.logging_max_backups(), 3);
    assert_eq!(snapshot.logging_retention_days(), 14);
}

#[test]
fn test_resolve_context_size_cli_wins() {
    let mut config = Config::default();
    config.model.context_size = 32000;
    let snap = ConfigSnapshot::new(config);
    assert_eq!(snap.resolve_context_size(Some(64000), 0), 64000);
}

#[test]
fn test_resolve_context_size_snapshot_wins() {
    let mut config = Config::default();
    config.model.context_size = 32000;
    let snap = ConfigSnapshot::new(config);
    assert_eq!(snap.resolve_context_size(None, 0), 32000);
}

#[test]
fn test_resolve_context_size_model_window_fallback() {
    let config = Config::default();
    let snap = ConfigSnapshot::new(config);
    assert_eq!(snap.resolve_context_size(None, 96000), 96000);
}

#[test]
fn test_resolve_context_size_default() {
    let config = Config::default();
    let snap = ConfigSnapshot::new(config);
    assert_eq!(snap.resolve_context_size(None, 0), 128_000);
}

#[test]
fn test_resolve_context_size_cli_zero_ignored() {
    let mut config = Config::default();
    config.model.context_size = 32000;
    let snap = ConfigSnapshot::new(config);
    assert_eq!(snap.resolve_context_size(Some(0), 0), 32000);
}

/// #1626：snapshot context_size 明显小于 model registry 真实窗口
/// （< 50%）时视为疑似误配——返回值仍以 snapshot 为准（尊重显式配置），
/// 但必须留下可观测提示（warn + 独立可测的 hint 方法）。
#[test]
fn test_context_size_mismatch_hint_flags_suspiciously_small_window() {
    let mut config = Config::default();
    config.model.context_size = 8192;
    let snap = ConfigSnapshot::new(config);

    // 8192 < 200_000 / 2 → 疑似误配，hint 带方向与两窗口值
    assert_eq!(
        snap.context_size_mismatch_hint(200_000),
        Some(ContextSizeMisalignment::ConfiguredTooSmall {
            configured: 8192,
            registry: 200_000
        })
    );
    // 接近真实窗口（8192 ≥ 16384/2，不低于 50%）不提示
    assert_eq!(snap.context_size_mismatch_hint(16_384), None);
    // 未配置 snapshot 值（0 = 未设置）不提示
    let unset = ConfigSnapshot::new(Config::default());
    assert_eq!(unset.context_size_mismatch_hint(200_000), None);
    // registry 窗口未知（0）不提示
    assert_eq!(snap.context_size_mismatch_hint(0), None);
}

/// #1686：对称方向——snapshot context_size 明显大于 registry 真实窗口
/// （> 200%）同样是疑似误配：summary 预算等按窗口比例的派生预算会随
/// 配置膨胀，超出模型真实承载。返回值仍以 snapshot 为准，仅留下提示。
#[test]
fn test_context_size_mismatch_hint_flags_suspiciously_large_window() {
    let mut config = Config::default();
    config.model.context_size = 1_048_576;
    let snap = ConfigSnapshot::new(config);

    // 1M > 200_000 × 2 → 疑似配置残留，hint 指出过大方向
    assert_eq!(
        snap.context_size_mismatch_hint(200_000),
        Some(ContextSizeMisalignment::ConfiguredTooLarge {
            configured: 1_048_576,
            registry: 200_000
        })
    );
    // 恰好 2 倍（400_000 = 200_000 × 2）不算明显失配，不提示
    let mut boundary = Config::default();
    boundary.model.context_size = 400_000;
    let boundary_snap = ConfigSnapshot::new(boundary);
    assert_eq!(boundary_snap.context_size_mismatch_hint(200_000), None);
    // 未配置 / registry 未知同样不提示
    let unset = ConfigSnapshot::new(Config::default());
    assert_eq!(unset.context_size_mismatch_hint(200_000), None);
    assert_eq!(snap.context_size_mismatch_hint(0), None);
}

#[test]
fn test_substructure_accessors_return_config_fields() {
    let config = Config::default();
    let snap = ConfigSnapshot::new(config);
    // 子结构 accessor 应返回 snapshot 内部 Config 对应字段的引用
    assert_eq!(snap.models().default, Config::default().models.default);
    assert_eq!(
        snap.agents().max_concurrency,
        Config::default().agents.max_concurrency
    );
    assert_eq!(
        snap.hooks().events.len(),
        Config::default().hooks.events.len()
    );
    assert_eq!(snap.memory().enabled, Config::default().memory.enabled);
    assert_eq!(snap.skills().dirs, Config::default().skills.dirs);
    assert_eq!(snap.logging_level(), Config::default().logging.level);
}

#[test]
fn test_resolve_model_selection_returns_resolved() {
    let mut config = Config::default();
    config.models.default = "zhipu/glm-5.1".to_string();
    config.models.providers.insert(
        "zhipu".to_string(),
        ProviderModelsConfig {
            driver: "zhipu".to_string(),
            models: vec![ModelEntryConfig {
                id: "glm-5.1".to_string(),
                name: "GLM 5.1".to_string(),
                context_window: 128_000,
                max_tokens: 4096,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let snap = ConfigSnapshot::new(config);
    let resolved = snap.resolve_model_selection("zhipu/glm-5.1");
    let resolved = resolved.expect("zhipu/glm-5.1 应解析成功");
    assert_eq!(resolved.source_key, "zhipu");
    assert_eq!(resolved.model.id, "glm-5.1");
    assert_eq!(resolved.driver, "zhipu");
}

#[test]
fn test_resolve_model_selection_unknown_source_errors() {
    let config = Config::default();
    let snap = ConfigSnapshot::new(config);
    assert!(snap.resolve_model_selection("unknown/model").is_err());
}

#[test]
fn test_list_models_returns_provider_entries() {
    let mut config = Config::default();
    config.models.providers.insert(
        "zhipu".to_string(),
        ProviderModelsConfig {
            driver: "zhipu".to_string(),
            models: vec![
                ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    ..Default::default()
                },
                ModelEntryConfig {
                    id: "glm-5.2".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    );
    let snap = ConfigSnapshot::new(config);
    let entries = snap.list_models();
    assert_eq!(entries.len(), 2, "应返回两个 model entry");
    let ids: Vec<&str> = entries.iter().map(|(_, m)| m.id.as_str()).collect();
    assert!(ids.contains(&"glm-5.1"));
    assert!(ids.contains(&"glm-5.2"));
}

// ── PR-C: from_args snapshot accessor 组合测试 ──────────────────────
//
// 以下测试模拟 from_args.rs 中消费方拿到 ConfigSnapshot 后调 accessor
// 的场景，验证非默认配置值能正确透传。

/// Config.model.context_size=32000 时，消费方调 snapshot.context_size() 应得 32000。
#[test]
fn test_snapshot_context_size_priority() {
    // Arrange
    let mut config = Config::default();
    config.model.context_size = 32000;
    let snap = ConfigSnapshot::new(config);

    // Act & Assert
    assert_eq!(snap.context_size(), 32000);
}

#[test]
fn snapshot_auto_compact_failure_limit_defaults_and_normalizes_zero() {
    let default_snapshot = ConfigSnapshot::new(Config::default());
    assert_eq!(default_snapshot.auto_compact_failure_limit(), 3);

    let mut config = Config::default();
    config.context.auto_compact_failure_limit = 0;
    let normalized_snapshot = ConfigSnapshot::new(config);
    assert_eq!(normalized_snapshot.auto_compact_failure_limit(), 1);
}

/// Config.model.max_tokens=8192 时，消费方调 snapshot.max_tokens() 应得 8192。
#[test]
fn test_snapshot_max_tokens() {
    // Arrange
    let mut config = Config::default();
    config.model.max_tokens = 8192;
    let snap = ConfigSnapshot::new(config);

    // Act & Assert
    assert_eq!(snap.max_tokens(), 8192);
}

#[test]
fn test_snapshot_max_tokens_zero_uses_default() {
    let mut config = Config::default();
    config.model.max_tokens = 0;
    let snap = ConfigSnapshot::new(config);

    assert_eq!(snap.max_tokens(), crate::config::models::DEFAULT_MAX_TOKENS);
}

#[test]
fn test_snapshot_resolve_runtime_model_model_wins_over_config() {
    let mut config = Config::default();
    config.model.max_tokens = 200_000;
    config.models.default = "zhipu/glm-5.1".to_string();
    config.models.providers.insert(
        "zhipu".to_string(),
        ProviderModelsConfig {
            driver: "zhipu".to_string(),
            models: vec![ModelEntryConfig {
                id: "glm-5.1".to_string(),
                name: "GLM 5.1".to_string(),
                context_window: 128_000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let snap = ConfigSnapshot::new(config);

    let runtime_model = snap.resolve_runtime_model(None, None).unwrap();

    assert_eq!(runtime_model.max_tokens(), 8192);
    assert_eq!(
        runtime_model.max_tokens_source(),
        crate::config::models::MaxTokensSource::Model
    );
}

#[test]
fn test_snapshot_resolve_runtime_model_cli_wins() {
    let mut config = Config::default();
    config.model.max_tokens = 200_000;
    config.models.default = "zhipu/glm-5.1".to_string();
    config.models.providers.insert(
        "zhipu".to_string(),
        ProviderModelsConfig {
            driver: "zhipu".to_string(),
            models: vec![ModelEntryConfig {
                id: "glm-5.1".to_string(),
                name: "GLM 5.1".to_string(),
                context_window: 128_000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let snap = ConfigSnapshot::new(config);

    let runtime_model = snap.resolve_runtime_model(None, Some(4096)).unwrap();

    assert_eq!(runtime_model.max_tokens(), 4096);
    assert_eq!(
        runtime_model.max_tokens_source(),
        crate::config::models::MaxTokensSource::Cli
    );
}

#[test]
fn test_snapshot_resolve_runtime_model_cli_zero_errors() {
    let mut config = Config::default();
    config.models.default = "zhipu/glm-5.1".to_string();
    config.models.providers.insert(
        "zhipu".to_string(),
        ProviderModelsConfig {
            driver: "zhipu".to_string(),
            models: vec![ModelEntryConfig {
                id: "glm-5.1".to_string(),
                name: "GLM 5.1".to_string(),
                context_window: 128_000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let snap = ConfigSnapshot::new(config);

    let err = snap.resolve_runtime_model(None, Some(0)).unwrap_err();

    assert_eq!(
        err,
        crate::config::models::RuntimeModelResolutionError::CliMaxTokensZero
    );
}

/// Config 含 tools.max_concurrency=8 / agents.max_concurrency=4 时，
/// 消费方调对应 accessor 应得正确值。
#[test]
fn test_snapshot_concurrency_limits() {
    // Arrange
    let mut config = Config::default();
    config.tools.max_concurrency = 8;
    config.agents.max_concurrency = 4;
    let snap = ConfigSnapshot::new(config);

    // Act & Assert
    assert_eq!(snap.max_tool_concurrency(), 8);
    assert_eq!(snap.max_agent_concurrency(), 4);
}

#[test]
fn snapshot_concurrency_limits_use_domain_defaults_for_default_config() {
    let snap = ConfigSnapshot::new(Config::default());

    assert_eq!(snap.max_tool_concurrency(), 10);
    assert_eq!(snap.max_agent_concurrency(), 4);
}

#[test]
fn snapshot_concurrency_limits_normalize_zero_to_domain_defaults() {
    let mut config = Config::default();
    config.tools.max_concurrency = 0;
    config.agents.max_concurrency = 0;
    let snap = ConfigSnapshot::new(config);

    assert_eq!(snap.max_tool_concurrency(), 10);
    assert_eq!(snap.max_agent_concurrency(), 4);
}

#[test]
fn snapshot_exposes_validated_tool_result_policy() {
    let mut config = Config::default();
    config.tools.tool_result.threshold_chars = 8_000;
    config.tools.tool_result.preview_head_chars = 1_000;
    config.tools.tool_result.preview_tail_chars = 250;
    let snap = ConfigSnapshot::new(config);

    let policy = snap.tool_result_policy(1_000_000);
    assert_eq!(policy.threshold_chars(), 8_000);
    assert_eq!(policy.preview_head_chars(), 1_000);
    assert_eq!(policy.preview_tail_chars(), 250);
}

#[test]
fn snapshot_normalizes_invalid_tool_result_policy_to_compatible_defaults() {
    let mut config = Config::default();
    config.tools.tool_result.threshold_chars = 0;
    config.tools.tool_result.preview_head_chars = 9_000;
    config.tools.tool_result.preview_tail_chars = 9_000;
    let snap = ConfigSnapshot::new(config);

    let policy = snap.tool_result_policy(1_000_000);
    assert_eq!(policy.threshold_chars(), 50_000);
    assert_eq!(policy.preview_head_chars(), 2_000);
    assert_eq!(policy.preview_tail_chars(), 500);
}

/// tool_result 截断阈值必须随 context window 比例收紧：
/// `threshold = min(配置值, 窗口×5%)`，下限 4k chars；
/// head/tail 等比收紧（threshold 的 1/4、1/8）且收紧后仍满足
/// `head + tail ≤ threshold` 不变式；窗口未知（0）时不收紧。
/// 配置值语义是"大窗口下的上限"——1M 窗口下默认 50k 占 5% 合理，
/// 128k 窗口下单条 50k chars（中文场景约 50k tokens）即占 40%，
/// 会直接把启发式估算顶到 auto-compact 阈值。
#[test]
fn tool_result_policy_scales_threshold_with_context_window() {
    let snap = ConfigSnapshot::new(Config::default());

    // 1M 窗口：5% = 50k，与默认配置相等，不收紧
    let policy = snap.tool_result_policy(1_000_000);
    assert_eq!(policy.threshold_chars(), 50_000);
    assert_eq!(policy.preview_head_chars(), 2_000);
    assert_eq!(policy.preview_tail_chars(), 500);

    // 200k 窗口：5% = 10k 收紧；head/tail 低于等比上限，保持原值
    let policy = snap.tool_result_policy(200_000);
    assert_eq!(policy.threshold_chars(), 10_000);
    assert_eq!(policy.preview_head_chars(), 2_000);
    assert_eq!(policy.preview_tail_chars(), 500);

    // 128k 窗口：5% = 6.4k；head 收紧到 6400/4 = 1600
    let policy = snap.tool_result_policy(128_000);
    assert_eq!(policy.threshold_chars(), 6_400);
    assert_eq!(policy.preview_head_chars(), 1_600);
    assert_eq!(policy.preview_tail_chars(), 500);

    // 32k 窗口：5% = 1600 低于下限，取 4k；head = 4000/4 = 1000
    let policy = snap.tool_result_policy(32_000);
    assert_eq!(policy.threshold_chars(), 4_000);
    assert_eq!(policy.preview_head_chars(), 1_000);
    assert_eq!(policy.preview_tail_chars(), 500);

    // 窗口未知（0）：不收紧，避免误伤
    let policy = snap.tool_result_policy(0);
    assert_eq!(policy.threshold_chars(), 50_000);
    assert_eq!(policy.preview_head_chars(), 2_000);
    assert_eq!(policy.preview_tail_chars(), 500);
}

/// resolve_context_size 在 CLI 传 0 时应忽略 CLI（用 snapshot 值），
/// CLI 传 128000 时应直接使用 CLI 值。
#[test]
fn test_snapshot_resolve_context_size_with_model_window() {
    // Arrange — snapshot 值为 32000，model_window 为 96000
    let mut config = Config::default();
    config.model.context_size = 32000;
    let snap = ConfigSnapshot::new(config);

    // Act & Assert — CLI 0 被忽略，回退到 snapshot 32000
    assert_eq!(snap.resolve_context_size(Some(0), 96000), 32000);

    // Act & Assert — CLI 128000 覆盖 snapshot
    assert_eq!(snap.resolve_context_size(Some(128000), 96000), 128000);
}

/// Config 只暴露仍受支持的 memory 子结构。
#[test]
fn test_snapshot_memory_accessor() {
    let mut config = Config::default();
    config.memory.enabled = true;
    let snap = ConfigSnapshot::new(config);

    assert!(snap.memory().enabled, "memory().enabled 应为 true");
}

#[test]
fn retired_reasoning_graph_section_is_ignored_by_config() {
    let config: Config = serde_json::from_value(serde_json::json!({
        "reasoning_graph": {
            "enabled": true,
            "max_reasoning": "high",
            "nodes": { "plan": { "effort": "low" } }
        }
    }))
    .expect("unknown retired section should remain backward-readable");
    let serialized = serde_json::to_value(config).expect("config serializes");
    assert!(serialized.get("reasoning_graph").is_none());
}

/// Config.language="zh" 时，snapshot.language() 应返回 "zh"。
#[test]
fn test_snapshot_language() {
    // Arrange
    let config = Config {
        language: "zh".to_string(),
        ..Config::default()
    };
    let snap = ConfigSnapshot::new(config);

    // Act & Assert
    assert_eq!(snap.language(), "zh");
}

/// Default Config 的 language 应为 "en"。
#[test]
fn test_snapshot_language_default() {
    // Arrange
    let config = Config::default();
    let snap = ConfigSnapshot::new(config);

    // Act & Assert
    assert_eq!(snap.language(), "en");
}
