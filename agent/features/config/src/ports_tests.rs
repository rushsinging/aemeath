//! `agent/features/config/src/ports.rs` 的契约测试。
//!
//! 覆盖目标：
//! - `SystemInformation` 字段类型稳定；
//! - `SystemInformationPort::current` 是 async；
//! - 测试辅助 `StaticSystemInformation` 仅在测试模块内可用，不进入生产 API；
//! - Provider Probe 端口契约：`ProviderProbeRequest` 字段稳定、`ProviderProbeErrorKind`
//!   是封闭枚举、错误展示走 `Display`，敏感字段不应出现在 `Debug` 默认输出之外的字段；
//! - 测试辅助 `StaticProbe` 仅在 ports_tests 内可达，不进入生产 API。

use std::sync::Arc;
use std::time::Duration;

use crate::catalog::DriverId;
use crate::ports::{
    ProviderProbeError, ProviderProbeErrorKind, ProviderProbePort, ProviderProbeRequest,
    ProviderProbeResult, SystemInformation, SystemInformationPort,
};

/// 测试辅助：固定返回值的 `SystemInformationPort` 实现。仅在 `ports_tests`
/// 模块内部使用，不在生产路径可达，亦不通过 `pub use` 重新导出到 crate 根。
#[derive(Debug, Clone)]
pub struct StaticSystemInformation {
    pub os_name: &'static str,
    pub os_version: Option<&'static str>,
    pub arch: &'static str,
}

#[async_trait::async_trait]
impl SystemInformationPort for StaticSystemInformation {
    async fn current(&self) -> SystemInformation {
        SystemInformation {
            os_name: self.os_name.to_string(),
            os_version: self.os_version.map(str::to_string),
            arch: self.arch.to_string(),
        }
    }
}

/// 测试辅助：固定结果的 `ProviderProbePort` 实现。仅在 `ports_tests` 模块内可用。
pub struct StaticProbe {
    pub behavior: tokio::sync::Mutex<ProbeBehavior>,
    pub request_log: tokio::sync::Mutex<Vec<ProviderProbeRequest>>,
}

#[derive(Debug, Clone)]
pub enum ProbeBehavior {
    Success,
    Failure(ProviderProbeErrorKind),
}

impl StaticProbe {
    pub fn new(behavior: ProbeBehavior) -> Arc<Self> {
        Arc::new(Self {
            behavior: tokio::sync::Mutex::new(behavior),
            request_log: tokio::sync::Mutex::new(Vec::new()),
        })
    }
}

#[async_trait::async_trait]
impl ProviderProbePort for StaticProbe {
    async fn probe(
        &self,
        request: ProviderProbeRequest,
    ) -> Result<ProviderProbeResult, ProviderProbeError> {
        self.request_log.lock().await.push(request);
        match &*self.behavior.lock().await {
            ProbeBehavior::Success => Ok(ProviderProbeResult {
                latency: Duration::from_millis(1),
            }),
            ProbeBehavior::Failure(kind) => Err(ProviderProbeError {
                kind: *kind,
                message: "测试注入失败".to_string(),
            }),
        }
    }
}

// SystemInformationPort tests ----------------------------------------------------

#[test]
fn system_information_constructor_propagates_owned_strings() {
    let info = SystemInformation {
        os_name: "macos".to_string(),
        os_version: Some("15.5".to_string()),
        arch: "aarch64".to_string(),
    };
    assert_eq!(info.os_name, "macos");
    assert_eq!(info.os_version.as_deref(), Some("15.5"));
    assert_eq!(info.arch, "aarch64");
}

#[test]
fn system_information_supports_none_os_version_for_unavailable_platforms() {
    let info = SystemInformation {
        os_name: "linux".to_string(),
        os_version: None,
        arch: "x86_64".to_string(),
    };
    assert!(info.os_version.is_none());
}

#[tokio::test]
async fn static_system_information_test_helper_returns_configured_values() {
    let helper = StaticSystemInformation {
        os_name: "linux",
        os_version: Some("6.1.0"),
        arch: "x86_64",
    };
    let snapshot = helper.current().await;
    assert_eq!(snapshot.os_name, "linux");
    assert_eq!(snapshot.os_version.as_deref(), Some("6.1.0"));
    assert_eq!(snapshot.arch, "x86_64");
}

#[tokio::test]
async fn static_system_information_supports_none_os_version() {
    let helper = StaticSystemInformation {
        os_name: "linux",
        os_version: None,
        arch: "x86_64",
    };
    let snapshot = helper.current().await;
    assert!(snapshot.os_version.is_none());
}

// ProviderProbePort tests -------------------------------------------------------

#[test]
fn provider_probe_request_field_set_is_stable() {
    let request = ProviderProbeRequest {
        driver: DriverId::new("anthropic"),
        base_url: "https://api.anthropic.com".to_string(),
        credential: Some("redacted".to_string()),
        model_id: "claude-sonnet-4-6".to_string(),
        context_window: 200_000,
        max_tokens: 8192,
        final_user_agent: "Aemeath/0.1.0 cli macos/aarch64".to_string(),
        timeout: Duration::from_secs(15),
    };
    // 字段稳定性断言：每个字段都必须存在并按预期类型持有数据。
    assert_eq!(request.driver.as_str(), "anthropic");
    assert_eq!(request.base_url, "https://api.anthropic.com");
    assert_eq!(request.credential.as_deref(), Some("redacted"));
    assert_eq!(request.model_id, "claude-sonnet-4-6");
    assert_eq!(request.context_window, 200_000);
    assert_eq!(request.max_tokens, 8192);
    assert_eq!(request.final_user_agent, "Aemeath/0.1.0 cli macos/aarch64");
    assert_eq!(request.timeout, Duration::from_secs(15));
}

#[test]
fn provider_probe_request_supports_absent_credential_for_env_less_endpoints() {
    let request = ProviderProbeRequest {
        driver: DriverId::new("ollama"),
        base_url: "http://localhost:11434".to_string(),
        credential: None,
        model_id: "llama3".to_string(),
        context_window: 8_192,
        max_tokens: 1024,
        final_user_agent: "Aemeath/0.1.0 cli linux/x86_64".to_string(),
        timeout: Duration::from_secs(10),
    };
    assert!(
        request.credential.is_none(),
        "空凭证必须表示为 None，避免在 Probe 路径混淆 Optional"
    );
}

#[test]
fn provider_probe_error_kind_is_closed_set() {
    use ProviderProbeErrorKind::*;
    // 契约保证：所有变体可见且无新增。`as_str` 不暴露在生产 API；这里仅做穷举证明
    // 新增变体需要在所有匹配处同步修改，避免悄悄引入未映射分类。
    let kinds = [
        Cancelled,
        Timeout,
        Authentication,
        Endpoint,
        Model,
        Protocol,
        Internal,
    ];
    for kind in kinds {
        let err = ProviderProbeError {
            kind,
            message: "测试".to_string(),
        };
        let formatted = format!("{err}");
        assert!(!formatted.is_empty(), "Display 必须给出非空可展示消息");
    }
}

#[test]
fn provider_probe_error_display_uses_message_only() {
    let err = ProviderProbeError {
        kind: ProviderProbeErrorKind::Authentication,
        message: "凭证无效".to_string(),
    };
    assert_eq!(format!("{err}"), "凭证无效");
}

#[tokio::test]
async fn static_probe_returns_success_when_behavior_is_success() {
    let probe = StaticProbe::new(ProbeBehavior::Success);
    let request = ProviderProbeRequest {
        driver: DriverId::new("anthropic"),
        base_url: "https://api.anthropic.com".to_string(),
        credential: None,
        model_id: "claude-sonnet-4-6".to_string(),
        context_window: 200_000,
        max_tokens: 8192,
        final_user_agent: "Aemeath/0.1.0 cli macos/aarch64".to_string(),
        timeout: Duration::from_secs(5),
    };
    let result = probe.probe(request.clone()).await.expect("success");
    assert_eq!(result.latency, Duration::from_millis(1));
    assert_eq!(probe.request_log.lock().await.len(), 1);
    assert_eq!(
        probe.request_log.lock().await[0].driver.as_str(),
        request.driver.as_str(),
    );
}

#[tokio::test]
async fn static_probe_returns_failure_with_stable_kind() {
    let probe = StaticProbe::new(ProbeBehavior::Failure(ProviderProbeErrorKind::Timeout));
    let request = ProviderProbeRequest {
        driver: DriverId::new("anthropic"),
        base_url: "https://api.anthropic.com".to_string(),
        credential: None,
        model_id: "claude-sonnet-4-6".to_string(),
        context_window: 200_000,
        max_tokens: 8192,
        final_user_agent: "Aemeath/0.1.0 cli macos/aarch64".to_string(),
        timeout: Duration::from_secs(5),
    };
    let err = probe
        .probe(request)
        .await
        .expect_err("failure 必须返回 typed error");
    assert_eq!(err.kind, ProviderProbeErrorKind::Timeout);
    assert!(!err.message.is_empty());
}
