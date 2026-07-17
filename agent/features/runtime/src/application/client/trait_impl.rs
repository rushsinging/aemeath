//! AgentClient trait 实现 — 薄委托到各子模块。

use async_trait::async_trait;
use sdk::{AgentClient, ChatRequest, ChatStream, SdkError};

use super::accessors::AgentClientImpl;

#[async_trait]
impl AgentClient for AgentClientImpl {
    async fn config_view(&self) -> Result<sdk::ConfigView, SdkError> {
        let snapshot = self
            .inner
            .config_query
            .snapshot()
            .await
            .map_err(|error| SdkError::Internal(format!("配置查询失败：{error:?}")))?;
        Ok(super::mapping::config_snapshot_to_sdk(&snapshot))
    }

    async fn update_config(
        &self,
        update: sdk::ConfigUpdate,
    ) -> Result<sdk::ConfigUpdateResult, SdkError> {
        let command = match update {
            sdk::ConfigUpdate::SetModel { model } => config::ConfigUpdate::SetModel { model },
            sdk::ConfigUpdate::SetPermissionMode { mode } => {
                config::ConfigUpdate::SetPermissionMode {
                    mode: match mode {
                        sdk::PermissionModeView::Ask => share::config::PermissionModeConfig::Ask,
                        sdk::PermissionModeView::AutoRead => {
                            share::config::PermissionModeConfig::AutoRead
                        }
                        sdk::PermissionModeView::AllowAll => {
                            share::config::PermissionModeConfig::AllowAll
                        }
                    },
                }
            }
        };
        let change = self
            .inner
            .config_writer
            .update(command)
            .await
            .map_err(|error| SdkError::Internal(format!("配置更新失败：{error:?}")))?;
        Ok(super::mapping::config_change_to_sdk(change))
    }

    fn cancel_run(&self, run_id: &sdk::RunId) -> sdk::CancelRunOutcome {
        self.inner.active_run.cancel(run_id)
    }

    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, SdkError> {
        super::trait_chat::chat_impl(self, input).await
    }
}
