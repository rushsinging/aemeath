use std::sync::Arc;

use sdk::SdkError;
use share::i18n::runtime::command as t;

use super::accessors::AgentClientImpl;

type Result<T> = std::result::Result<T, SdkError>;

pub(super) async fn execute_command_impl(
    me: &AgentClientImpl,
    name: &str,
    args: &str,
    sdk_ctx: sdk::CommandContext,
) -> Result<sdk::CommandResult> {
    use crate::business::cost::CostTracker;
    use crate::business::state::AppState;
    use crate::core::command::CommandContext as RtCmdCtx;
    use share::config::Config;

    let lang = &me.inner.context.resources.language;

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
            Ok(sdk::CommandResult::Error(t::unknown_command(
                lang, &cmd_name,
            )))
        }
        None => Ok(sdk::CommandResult::Error(t::unknown_command(
            lang, &cmd_name,
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
    let context_size = me.inner.context.resources.context_size;
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
