//! HookRunner 适配器——为 HookNotificationPort 提供对 hook::HookRunner 的封装。
//!
//! 由于 orphan rule，runtime 无法直接为 hook crate 的类型实现 port trait，
//! 使用 newtype wrapper 解决。

use crate::core::port::HookNotificationPort;

/// HookRunner 的 newtype 适配器，封装 hook::HookRunner 的通知发送方法。
pub struct HookRunnerAdapter(pub hook::api::HookRunner);

impl HookRunnerAdapter {
    pub fn new(runner: hook::api::HookRunner) -> Self {
        Self(runner)
    }
}

#[async_trait::async_trait]
impl HookNotificationPort for HookRunnerAdapter {
    async fn on_notification(&self, message: &str, kind: &str) {
        let _ = self.0.on_notification(message, kind).await;
    }
}
