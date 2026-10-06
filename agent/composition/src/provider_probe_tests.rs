use super::*;
use config::ports::{ProviderProbeErrorKind, ProviderProbeRequest};

fn request() -> ProviderProbeRequest {
    ProviderProbeRequest {
        driver: config::catalog::find_by_source("Anthropic").unwrap().driver,
        base_url: "https://probe.test".to_string(),
        credential: Some("sk-probe-secret".to_string()),
        model_id: "probe-model".to_string(),
        context_window: 32_000,
        max_tokens: 64,
        final_user_agent: "probe-agent/1.0".to_string(),
        timeout: std::time::Duration::from_secs(9),
        api_style: None,
    }
}

/// 请求 → provider 构造配置的翻译语义：单 token 探测、秒级超时
/// （亚秒抬到 1s）、独立 source_key、透传凭据与 UA。
#[test]
fn probe_config_from_request_applies_probe_semantics() {
    let config = probe_config_from_request(&request());
    assert_eq!(config.driver, "anthropic");
    assert_eq!(config.source_key, "connect-probe");
    assert_eq!(config.api_key, "sk-probe-secret");
    assert_eq!(config.base_url.as_deref(), Some("https://probe.test"));
    assert_eq!(config.model, "probe-model");
    assert_eq!(config.max_tokens, 1);
    assert!(!config.reasoning);
    assert_eq!(config.timeout_secs, 9);
    assert_eq!(config.user_agent.as_deref(), Some("probe-agent/1.0"));
}

#[test]
fn probe_config_subsecond_timeout_floors_to_one_second() {
    let mut probe_request = request();
    probe_request.timeout = std::time::Duration::from_millis(150);
    assert_eq!(probe_config_from_request(&probe_request).timeout_secs, 1);
}

/// ProviderError → ProviderProbeError 的分类稳定性与脱敏（上游 wire
/// 细节 NEVER 进入向导文案）。
#[test]
fn provider_error_mapping_is_stable_and_redacted() {
    let cases = [
        (
            ProviderErrorKind::Cancelled,
            ProviderProbeErrorKind::Cancelled,
        ),
        (ProviderErrorKind::Timeout, ProviderProbeErrorKind::Timeout),
        (
            ProviderErrorKind::Authentication,
            ProviderProbeErrorKind::Authentication,
        ),
        (
            ProviderErrorKind::PermissionDenied,
            ProviderProbeErrorKind::Authentication,
        ),
        (
            ProviderErrorKind::ModelUnavailable,
            ProviderProbeErrorKind::Model,
        ),
        (
            ProviderErrorKind::Protocol,
            ProviderProbeErrorKind::Protocol,
        ),
        (ProviderErrorKind::Network, ProviderProbeErrorKind::Endpoint),
    ];
    for (source, expected) in cases {
        let error = map_probe_error(ProviderError::fatal(source, "sensitive wire body"));
        assert_eq!(error.kind, expected);
        assert!(!error.message.contains("wire body"));
    }
}
