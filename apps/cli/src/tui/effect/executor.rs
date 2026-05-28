use super::{effect::Effect, result::EffectResult};

#[derive(Default)]
pub struct EffectExecutor;

impl EffectExecutor {
    pub async fn execute(&mut self, effect: Effect) -> EffectResult {
        match effect {
            Effect::None => EffectResult::Noop,
            Effect::RequestRender => EffectResult::RenderRequested,
            Effect::SpawnAgentChat { chat_id, .. } => EffectResult::AgentChatSpawned { chat_id },
            Effect::SaveSession => EffectResult::SessionSaved,
            Effect::FetchTaskStatus => EffectResult::TaskStatusFetched,
            Effect::CopyToClipboard { .. } => EffectResult::ClipboardCopied,
            Effect::RunHook { name } => EffectResult::HookFinished {
                name,
                success: true,
            },
            Effect::StartTimer { id } => EffectResult::TimerStarted { id },
            Effect::StopTimer { id } => EffectResult::TimerStopped { id },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::effect::effect::Effect;
    use crate::tui::effect::result::EffectResult;

    #[tokio::test]
    async fn test_executor_handles_request_render_without_io() {
        let mut executor = EffectExecutor;
        let result = executor.execute(Effect::RequestRender).await;
        assert_eq!(result, EffectResult::RenderRequested);
    }

    #[tokio::test]
    async fn test_executor_handles_none_as_noop() {
        let mut executor = EffectExecutor;
        let result = executor.execute(Effect::None).await;
        assert_eq!(result, EffectResult::Noop);
    }

    #[tokio::test]
    async fn test_executor_preserves_chat_id() {
        let mut executor = EffectExecutor;
        let result = executor
            .execute(Effect::SpawnAgentChat {
                chat_id: "chat-1".to_string(),
                prompt: "hello".to_string(),
            })
            .await;
        assert_eq!(
            result,
            EffectResult::AgentChatSpawned {
                chat_id: "chat-1".to_string()
            }
        );
    }
}
