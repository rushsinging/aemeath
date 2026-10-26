//! ScoringConfig 默认值、serde 与别名行为测试。
//!
//! HTTP 端点配置（url/model/timeout_ms）已随设计 §4.3 退役：仅保留四个场景开关，
//! 旧键残留配置必须被忽略而非报错。

use super::*;

#[test]
fn default_scoring_config_disables_all_scenarios() {
    let config = ScoringConfig::default();
    assert!(!config.memory_rerank);
    assert!(!config.skill_match);
    assert!(!config.policy_triage);
    assert!(!config.memory_recall);
}

#[test]
fn scoring_config_ignores_retired_http_keys() {
    let config: ScoringConfig = serde_json::from_str(
        r#"{"url":"http://10.0.0.2:9000","model":"kev-0.8b","timeoutMs":500,"memoryRerank":true}"#,
    )
    .expect("退役 HTTP 键应被忽略而非报错");
    assert!(config.memory_rerank, "仍生效的开关应正常解析");
    assert!(!config.skill_match);

    let value = serde_json::to_value(&config).expect("ScoringConfig 应可序列化");
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("对象")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "backend",
            "bailian",
            "connect_timeout_secs",
            "enabled",
            "event_retention_days",
            "http_endpoint",
            "http_listen",
            "infer_timeout_secs",
            "memory_recall",
            "memory_rerank",
            "policy_triage",
            "skill_match",
        ],
        "序列化面允许场景开关、事件保留、daemon socket/超时与 backend 驱动配置；退役 HTTP 顶层字段不得回归"
    );
}

#[test]
fn scoring_config_accepts_camel_case_aliases_for_switches() {
    let config: ScoringConfig = serde_json::from_str(
        r#"{"memoryRerank": true, "skillMatch": true, "policyTriage": true, "memoryRecall": true}"#,
    )
    .expect("camelCase 别名应可解析");
    assert!(config.memory_rerank);
    assert!(config.skill_match);
    assert!(config.policy_triage);
    assert!(config.memory_recall);
}

#[test]
fn empty_scoring_json_uses_defaults() {
    let config: ScoringConfig = serde_json::from_str(r#"{}"#).expect("空对象应解析");
    assert_eq!(config, ScoringConfig::default());
}

#[test]
fn event_retention_days_parses_from_scoring_section() {
    // 设计 03-event-stream §8：`scoring.event_retention_days` 配置贯通。
    let config: crate::config::Config =
        serde_json::from_str(r#"{"scoring":{"event_retention_days":7}}"#)
            .expect("scoring 配置段应可解析 event_retention_days");
    assert_eq!(config.scoring.event_retention_days, 7, "显式配置应生效");
}

#[test]
fn event_retention_days_defaults_to_thirty() {
    let from_empty: ScoringConfig = serde_json::from_str(r#"{}"#).expect("空对象应解析");
    assert_eq!(from_empty.event_retention_days, 30, "缺省保留 30 日");
    assert_eq!(
        ScoringConfig::default().event_retention_days,
        30,
        "Default 缺省与 serde 缺省必须同源同值"
    );
    assert_eq!(
        from_empty,
        ScoringConfig::default(),
        "缺省解析结果应等于 Default"
    );
}

#[test]
fn event_retention_days_zero_is_legal_gc_off_not_write_off() {
    let config: ScoringConfig = serde_json::from_str(r#"{"event_retention_days":0}"#)
        .expect("0 应合法（0=禁用 GC，不影响事件写入）");
    assert_eq!(config.event_retention_days, 0);
}

#[test]
fn scoring_config_defaults_daemon_timeouts_and_http_listen() {
    let config = ScoringConfig::default();
    assert_eq!(
        config.connect_timeout_secs,
        DEFAULT_SCORING_CONNECT_TIMEOUT_SECS
    );
    assert_eq!(
        config.infer_timeout_secs,
        DEFAULT_SCORING_INFER_TIMEOUT_SECS
    );
    assert_eq!(config.http_listen, DEFAULT_SCORING_HTTP_LISTEN);
    assert!(config.http_endpoint.is_none());
}

#[test]
fn scoring_config_accepts_http_listen_and_timeout_aliases() {
    let config: ScoringConfig = serde_json::from_str(
        r#"{
            "httpListen": "127.0.0.1:9100",
            "connectTimeoutSecs": 5,
            "inferTimeoutSecs": 60
        }"#,
    )
    .expect("deserialize");
    assert_eq!(config.http_listen, "127.0.0.1:9100");
    assert_eq!(config.connect_timeout_secs, 5);
    assert_eq!(config.infer_timeout_secs, 60);
}

#[test]
fn scoring_http_endpoint_defaults_from_listen() {
    let config = ScoringConfig::default();
    assert_eq!(
        config.resolve_http_endpoint(),
        format!("http://{DEFAULT_SCORING_HTTP_LISTEN}/systemone"),
        "client 端点缺省由 http_listen 推导（同机零配置）"
    );
}

#[test]
fn scoring_http_endpoint_explicit_value_wins() {
    let config: ScoringConfig =
        serde_json::from_str(r#"{"httpEndpoint": "https://scoring.example.com/systemone"}"#)
            .expect("显式 endpoint 解析");
    assert_eq!(
        config.resolve_http_endpoint(),
        "https://scoring.example.com/systemone"
    );
}

#[test]
fn scoring_socket_path_retired_key_ignored() {
    // UDS 退役（#2096 拍板）：旧 socket_path 残留键被忽略而非报错，
    // 序列化面不再出现该字段。
    let config: ScoringConfig = serde_json::from_str(
        r#"{"socketPath": "/tmp/legacy.sock", "socket_path": "/tmp/legacy2.sock"}"#,
    )
    .expect("退役键残留应被忽略");
    let value = serde_json::to_value(&config).expect("序列化");
    assert!(
        value.get("socket_path").is_none(),
        "序列化面不得再出现 socket_path"
    );
}

// ── backend 驱动选择（#2095：bailian 云端 backend）──

#[test]
fn scoring_backend_defaults_to_local() {
    assert_eq!(ScoringConfig::default().backend, ScoringBackend::Local);
    let from_empty: ScoringConfig = serde_json::from_str(r#"{}"#).expect("空对象应解析");
    assert_eq!(from_empty.backend, ScoringBackend::Local);
}

#[test]
fn scoring_backend_parses_bailian_and_rejects_unknown_values() {
    let config: ScoringConfig =
        serde_json::from_str(r#"{"backend":"bailian"}"#).expect("bailian 应可解析");
    assert_eq!(config.backend, ScoringBackend::Bailian);

    let config: ScoringConfig =
        serde_json::from_str(r#"{"backend":"local"}"#).expect("local 应可解析");
    assert_eq!(config.backend, ScoringBackend::Local);

    let result: Result<ScoringConfig, _> = serde_json::from_str(r#"{"backend":"ark"}"#);
    assert!(result.is_err(), "未知 backend 值必须拒绝而非静默回退");
}

#[test]
fn bailian_config_defaults_model_only() {
    let config = BailianScoringConfig::default();
    assert_eq!(config.model, DEFAULT_BAILIAN_SCORING_MODEL);
    assert!(config.api_key.is_none());
    assert!(config.base_url.is_none());
    assert!(config.full_url.is_none());
}

#[test]
fn bailian_config_accepts_camel_case_aliases() {
    let config: ScoringConfig = serde_json::from_str(
        r#"{
            "backend": "bailian",
            "bailian": {
                "apiKey": "sk-test",
                "baseUrl": "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
                "model": "decision-model-preview"
            }
        }"#,
    )
    .expect("camelCase 别名应可解析");
    assert_eq!(config.bailian.api_key.as_deref(), Some("sk-test"));
    assert_eq!(
        config.bailian.base_url.as_deref(),
        Some("https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1")
    );
    assert_eq!(config.bailian.model, "decision-model-preview");
}

#[test]
fn bailian_endpoint_prefers_full_url_over_base_url() {
    let config = BailianScoringConfig {
        full_url: Some("https://example.com/api/v1/systemone".into()),
        base_url: Some("https://other.com/compatible-mode/v1".into()),
        ..Default::default()
    };
    assert_eq!(
        config.resolve_endpoint().expect("full_url 优先"),
        "https://example.com/api/v1/systemone"
    );
}

#[test]
fn bailian_endpoint_joins_systemone_path_trimming_trailing_slash() {
    let config = BailianScoringConfig {
        base_url: Some(
            "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/".into(),
        ),
        ..Default::default()
    };
    assert_eq!(
        config.resolve_endpoint().expect("拼接应去尾斜杠"),
        "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/systemone"
    );
}

#[test]
fn bailian_endpoint_requires_at_least_one_url() {
    let config = BailianScoringConfig::default();
    assert!(
        config.resolve_endpoint().is_err(),
        "base_url 与 full_url 均缺时必须报错"
    );
}

#[test]
fn bailian_config_validate_requires_api_key_and_endpoint() {
    let no_key = BailianScoringConfig {
        base_url: Some("https://example.com/compatible-mode/v1".into()),
        ..Default::default()
    };
    assert!(no_key.validate().is_err(), "缺 api_key 必须校验失败");

    let no_url = BailianScoringConfig {
        api_key: Some("sk-test".into()),
        ..Default::default()
    };
    assert!(no_url.validate().is_err(), "缺端点必须校验失败");

    let complete = BailianScoringConfig {
        api_key: Some("sk-test".into()),
        base_url: Some("https://example.com/compatible-mode/v1".into()),
        ..Default::default()
    };
    assert!(complete.validate().is_ok(), "完整配置必须校验通过");
}

#[test]
fn scoring_config_serialization_includes_backend_and_bailian() {
    let config: ScoringConfig =
        serde_json::from_str(r#"{"backend":"bailian"}"#).expect("backend 应可解析");
    let value = serde_json::to_value(&config).expect("ScoringConfig 应可序列化");
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("对象")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "backend",
            "bailian",
            "connect_timeout_secs",
            "enabled",
            "event_retention_days",
            "http_endpoint",
            "http_listen",
            "infer_timeout_secs",
            "memory_recall",
            "memory_rerank",
            "policy_triage",
            "skill_match",
        ],
        "序列化面应含 backend 与 bailian 驱动配置"
    );
}
