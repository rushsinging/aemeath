//! LlmClient 适配器——为 ProviderInfoPort 提供对 provider::LlmClient 的封装。
//!
//! 由于 orphan rule，runtime 无法直接为 provider crate 的类型实现 port trait，
//! 使用 newtype wrapper 解决。

use crate::core::port::ProviderInfoPort;
use std::sync::Arc;

/// LlmClient 的 newtype 适配器，封装 provider::LlmClient 的元数据查询方法。
pub struct LlmClientAdapter(pub Arc<crate::api::provider::LlmClient>);

impl LlmClientAdapter {
    pub fn new(client: Arc<crate::api::provider::LlmClient>) -> Self {
        Self(client)
    }
}

impl ProviderInfoPort for LlmClientAdapter {
    fn provider_name(&self) -> &str {
        self.0.provider_name()
    }

    fn model_name(&self) -> &str {
        self.0.model_name()
    }

    fn is_reasoning(&self) -> bool {
        self.0.is_reasoning()
    }

    fn set_reasoning(&self, enabled: bool) {
        self.0.set_reasoning(enabled)
    }
}
