use std::sync::Arc;

use sdk::{ClipboardImageView, ModelSummary, ReflectionOutputView, SdkError};

use super::accessors::AgentClientImpl;
use crate::core::port::{HookNotificationPort, ProviderInfoPort};
use crate::utils::adapter::{HookRunnerAdapter, LlmClientAdapter};
use crate::utils::bootstrap::config_manager::ConfigManager;

type Result<T> = std::result::Result<T, SdkError>;

pub(super) async fn execute_command_impl(
    _me: &AgentClientImpl,
    name: &str,
    args: &str,
    sdk_ctx: sdk::CommandContext,
) -> Result<sdk::CommandResult> {
    use crate::business::cost::CostTracker;
    use crate::business::state::AppState;
    use crate::core::command::CommandContext as RtCmdCtx;
    use share::config::Config;

    // Build runtime command context
    let state = Arc::new(AppState::default());
    let config = Config::default();
    let mut cost_tracker = CostTracker::new();
    let _ = cost_tracker.load();

    let mut ctx = RtCmdCtx::new(state, config, sdk_ctx.cwd, sdk_ctx.session_id);
    ctx.current_model = sdk_ctx.current_model;
    ctx.models_config = share::config::ModelsConfig::default();

    // Scope: hold registry lock only for lookup, not across await
    let cmd_name = name.to_string();
    let args_owned = args.to_string();
    let result = {
        let registry = crate::core::command::CommandRegistry::global();
        registry.find(&cmd_name).map(|_cmd| {
            // Clone the name for later use in error messages
            (cmd_name.clone(), args_owned.clone())
        })
    };
    // Registry lock dropped here

    match result {
        Some(_) => {
            // Re-acquire for execution (separate lock)
            let registry = crate::core::command::CommandRegistry::global();
            if let Some(cmd) = registry.find(&cmd_name) {
                // The cmd reference outlives the guard because execute happens
                // within the scope. But we can't drop the guard before await.
                // Use block_in_place to make this Send-compatible.
                let result = tokio::task::block_in_place(|| {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(cmd.execute(&args_owned, &mut ctx))
                });
                return Ok(super::mapping::map_command_result(result));
            }
            Ok(sdk::CommandResult::Error(format!(
                "未知命令: /{}",
                cmd_name
            )))
        }
        None => Ok(sdk::CommandResult::Error(format!(
            "未知命令: /{}",
            cmd_name
        ))),
    }
}

pub(super) async fn estimate_context_impl(
    me: &AgentClientImpl,
    messages: &[sdk::ChatMessage],
    system_prompt: &str,
) -> Result<sdk::ContextEstimate> {
    let runtime_messages: Vec<share::message::Message> = messages
        .iter()
        .map(|msg| super::mapping::message_from_sdk(msg.clone()))
        .collect();
    let estimated = crate::business::compact::estimate_messages_tokens(&runtime_messages)
        + crate::business::compact::estimate_tokens(system_prompt);
    let context_size = me.inner.context.context_size;
    let pct = if context_size > 0 {
        estimated as f64 * 100.0 / context_size as f64
    } else {
        0.0
    };
    Ok(sdk::ContextEstimate {
        estimated_tokens: estimated,
        system_tokens: crate::business::compact::estimate_tokens(system_prompt),
        context_size,
        usage_percentage: pct,
    })
}

pub(super) async fn switch_model_impl(
    me: &AgentClientImpl,
    params: sdk::ModelSwitchParams,
) -> Result<sdk::ModelSwitchResult> {
    use provider::api::openai_compatible::ReasoningConfig;
    use provider::api::ApiDriverKind;

    let api_type = ApiDriverKind::parse(&params.api_type).unwrap_or(ApiDriverKind::OpenAI);
    let openai_config = switch_model_openai_config(api_type, &params.provider_name);

    let reasoning = params.reasoning.unwrap_or(true);
    let reasoning_config = Some(ReasoningConfig::Bool(reasoning));

    let new_client = provider::api::LlmClient::from_config(provider::api::LlmConfigOptions {
        api: api_type,
        api_key: params.api_key,
        base_url: Some(params.base_url),
        model: params.model_id.clone(),
        max_tokens: params.max_tokens,
        thinking_max_tokens: 0,
        reasoning,
        reasoning_config,
        openai_config,
    });

    let display_name = if params.model_name.is_empty() {
        &params.model_id
    } else {
        &params.model_name
    };
    let display = format!("{}/{}", params.provider_name, display_name);

    *me.inner.current_client.write().unwrap() = Arc::new(new_client);

    Ok(sdk::ModelSwitchResult {
        display_name: display,
        context_window: params.context_window,
        reasoning_active: Some(reasoning),
    })
}

pub(super) async fn set_thinking_impl(me: &AgentClientImpl, desired: Option<bool>) -> Result<bool> {
    let client = me.inner.current_client.read().unwrap().clone();
    let adapter = LlmClientAdapter::new(client);
    let current = adapter.is_reasoning();
    let new_state = desired.unwrap_or(!current);
    adapter.set_reasoning(new_state);
    Ok(new_state)
}

pub(super) async fn compact_messages_impl(
    me: &AgentClientImpl,
    messages: Vec<sdk::ChatMessage>,
    system_prompt: &str,
    context_size: usize,
) -> Result<(Vec<sdk::ChatMessage>, bool)> {
    let runtime_messages: Vec<share::message::Message> = messages
        .into_iter()
        .map(super::mapping::message_from_sdk)
        .collect();
    let client = me
        .inner
        .current_client
        .read()
        .ok()
        .map(|guard| (*guard).clone());
    let (compacted, was_compacted) = crate::business::compact::compact_messages_with_llm(
        &runtime_messages,
        system_prompt,
        context_size,
        client.as_ref().map(|c| c.as_ref()),
    )
    .await;
    let sdk_messages: Vec<sdk::ChatMessage> = compacted
        .into_iter()
        .map(super::mapping::message_to_sdk)
        .collect();
    Ok((sdk_messages, was_compacted))
}

pub(super) async fn notify_hook_impl(
    me: &AgentClientImpl,
    message: &str,
    kind: &str,
) -> Result<()> {
    if let Some(ref runner) = me.inner.hook_runner {
        let adapter = HookRunnerAdapter::new(runner.clone());
        adapter.on_notification(message, kind).await;
    }
    Ok(())
}

pub(super) async fn list_models_impl(me: &AgentClientImpl) -> Result<Vec<ModelSummary>> {
    let config = ConfigManager::new(Some(&me.inner.cwd))
        .load()
        .await
        .map_err(SdkError::Init)?;
    Ok(config
        .models
        .list_models()
        .into_iter()
        .map(|(provider, model)| ModelSummary {
            provider,
            id: model.id,
            name: model.name,
            context_window: model.context_window,
            max_tokens: model.max_tokens,
        })
        .collect())
}

pub(super) async fn compact_impl(_me: &AgentClientImpl) -> Result<()> {
    Ok(())
}

pub(super) async fn read_clipboard_image_impl(_me: &AgentClientImpl) -> Result<ClipboardImageView> {
    crate::utils::image::read_clipboard_image()
        .await
        .map(super::mapping::processed_image_to_sdk)
        .map_err(|e| SdkError::Internal(e.to_string()))
}

pub(super) async fn process_image_file_impl(
    _me: &AgentClientImpl,
    path: String,
) -> Result<ClipboardImageView> {
    crate::utils::image::process_image_file(&path)
        .await
        .map(super::mapping::processed_image_to_sdk)
        .map_err(|e| SdkError::Internal(e.to_string()))
}

pub(super) async fn run_reflection_impl(
    _me: &AgentClientImpl,
    messages: Vec<sdk::ChatMessage>,
) -> Result<ReflectionOutputView> {
    let runtime_messages = messages
        .into_iter()
        .map(super::mapping::message_from_sdk)
        .collect::<Vec<_>>();
    let recent_summary = crate::business::reflection::ReflectionEngine::recent_messages_summary(
        &runtime_messages,
        6000,
    );
    let output = crate::business::reflection::ReflectionOutput {
        deviations: vec![recent_summary],
        suggested_memories: Vec::new(),
        outdated_memories: Vec::new(),
        user_alert: None,
    };
    Ok(super::mapping::reflection_output_to_sdk(output, 0, 0))
}

pub(super) async fn apply_reflection_impl(
    _me: &AgentClientImpl,
    output: ReflectionOutputView,
) -> Result<String> {
    let count = output.suggested_memories.len();
    Ok(format!(
        "已生成 {count} 条记忆建议；自动写入将在后续 SDK memory 能力中接入"
    ))
}

pub(super) async fn list_reminders_impl(me: &AgentClientImpl) -> Result<Vec<sdk::ReminderView>> {
    let reminders = me.inner.session_reminders.read().unwrap();
    Ok(reminders
        .list()
        .iter()
        .map(|r| sdk::ReminderView {
            id: r.id.clone(),
            content: r.content.clone(),
            done: r.done,
            created_at: r.created_at,
        })
        .collect())
}

pub(super) async fn add_reminder_impl(me: &AgentClientImpl, content: &str) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    let created_at = current_timestamp_secs();
    me.inner
        .session_reminders
        .write()
        .unwrap()
        .add(id, content, created_at)
        .map_err(|e| SdkError::Internal(format!("添加 reminder 失败: {e}")))
}

fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn switch_model_openai_config(
    api_type: provider::api::ApiDriverKind,
    source_key: &str,
) -> Option<provider::api::OpenAIProviderConfig> {
    if matches!(
        api_type,
        provider::api::ApiDriverKind::Anthropic | provider::api::ApiDriverKind::Ollama
    ) {
        None
    } else {
        Some(provider::api::OpenAIProviderConfig::from_api_driver(
            api_type, source_key,
        ))
    }
}

pub(super) async fn complete_reminder_impl(me: &AgentClientImpl, id: &str) -> Result<()> {
    me.inner
        .session_reminders
        .write()
        .unwrap()
        .complete(id)
        .map_err(|e| SdkError::Internal(format!("完成 reminder 失败: {e}")))
}

pub(super) async fn get_thinking_impl(me: &AgentClientImpl) -> Result<bool> {
    let client = me.inner.current_client.read().unwrap().clone();
    let adapter = LlmClientAdapter::new(client);
    Ok(adapter.is_reasoning())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_switch_model_openai_config_skips_ollama() {
        let result = switch_model_openai_config(provider::api::ApiDriverKind::Ollama, "ollama");

        assert!(result.is_none());
    }

    #[test]
    fn test_switch_model_openai_config_skips_anthropic() {
        let result =
            switch_model_openai_config(provider::api::ApiDriverKind::Anthropic, "anthropic");

        assert!(result.is_none());
    }

    #[test]
    fn test_switch_model_openai_config_uses_source_key_for_openai_compatible() {
        let result = switch_model_openai_config(provider::api::ApiDriverKind::Zhipu, "Zhipu")
            .expect("zhipu should use openai-compatible config");

        assert_eq!(result.source_key, "Zhipu");
        assert_eq!(result.api, provider::api::ApiDriverKind::Zhipu);
    }
}
