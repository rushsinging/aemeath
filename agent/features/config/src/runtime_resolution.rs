//! Config-owned Provider Runtime Resolution。
//!
//! Runtime 只消费本模块返回的已解析值，不再自行读取 Provider Catalog、Provider
//! source 配置或拼接 User-Agent。

use crate::catalog::{find_by_driver, find_by_source};
use crate::ports::SystemInformation;
use crate::user_agent::{resolve_provider_user_agent_str, ProviderUserAgentInputs};
use share::config::domain::models::ResolvedModel;
use share::config::domain::snapshot::ConfigSnapshot;

/// 使用当前编译目标构造生产 Runtime resolver。
pub fn resolve_provider_runtime(
    snapshot: &ConfigSnapshot,
    resolved_model: &ResolvedModel,
    base_url_override: Option<&str>,
) -> ResolvedProviderRuntimeConfig {
    ProviderRuntimeResolver::new(
        SystemInformation {
            os_name: std::env::consts::OS.to_string(),
            os_version: None,
            arch: std::env::consts::ARCH.to_string(),
        },
        share::version(),
    )
    .resolve(snapshot, resolved_model, base_url_override)
}
/// 按 `source/model` selection 解析 Provider Runtime 配置。
///
/// 这是 Runtime 侧处理工具 UA 等同一 Run 配置消费的窄入口；未知 selection
/// 只返回全局 UA 回退结果，不暴露内部模型解析错误。
pub fn resolve_provider_runtime_for_selection(
    snapshot: &ConfigSnapshot,
    selection: &str,
    base_url_override: Option<&str>,
) -> ResolvedProviderRuntimeConfig {
    let resolver = ProviderRuntimeResolver::new(
        SystemInformation {
            os_name: std::env::consts::OS.to_string(),
            os_version: None,
            arch: std::env::consts::ARCH.to_string(),
        },
        share::version(),
    );
    match snapshot.resolve_model_selection(selection) {
        Ok(resolved_model) => resolver.resolve(snapshot, &resolved_model, base_url_override),
        Err(_) => ResolvedProviderRuntimeConfig {
            base_url: None,
            user_agent: resolver.resolve_global_user_agent(snapshot),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProviderRuntimeConfig {
    pub base_url: Option<String>,
    pub user_agent: String,
}

/// Provider 运行配置的唯一解析入口。
#[derive(Debug, Clone)]
pub struct ProviderRuntimeResolver {
    system: SystemInformation,
    version: String,
}

impl ProviderRuntimeResolver {
    pub fn new(system: SystemInformation, version: impl Into<String>) -> Self {
        Self {
            system,
            version: version.into(),
        }
    }

    fn resolve_global_user_agent(&self, snapshot: &ConfigSnapshot) -> String {
        resolve_provider_user_agent_str(ProviderUserAgentInputs {
            provider_user_agent: None,
            catalog_official_sdk_user_agent: None,
            global_user_agent: {
                let value = snapshot.user_agent().trim();
                (!value.is_empty()).then_some(value)
            },
            system: self.system.clone(),
            version: &self.version,
        })
    }

    pub fn resolve(
        &self,
        snapshot: &ConfigSnapshot,
        resolved_model: &ResolvedModel,
        base_url_override: Option<&str>,
    ) -> ResolvedProviderRuntimeConfig {
        let provider_base_url = non_empty(resolved_model.source_config.base_url.as_str());
        let catalog = find_by_source(&resolved_model.source_key)
            .or_else(|| find_by_driver(&resolved_model.driver));
        let base_url = base_url_override
            .and_then(non_empty)
            .or(provider_base_url)
            .or_else(|| {
                catalog.and_then(|entry| entry.default_endpoint.as_ref().map(|e| e.url.to_string()))
            });
        let catalog_user_agent = catalog
            .and_then(|entry| entry.official_sdk_user_agent.as_ref())
            .map(|value| value.value.clone());
        let user_agent = resolve_provider_user_agent_str(ProviderUserAgentInputs {
            provider_user_agent: resolved_model.source_config.normalized_user_agent(),
            catalog_official_sdk_user_agent: catalog_user_agent,
            global_user_agent: {
                let value = snapshot.user_agent().trim();
                (!value.is_empty()).then_some(value)
            },
            system: self.system.clone(),
            version: &self.version,
        });
        ResolvedProviderRuntimeConfig {
            base_url,
            user_agent,
        }
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
#[path = "runtime_resolution_tests.rs"]
mod tests;
