//! chat() 方法实际逻辑。

use std::sync::{Arc, Mutex};

use sdk::{ChatRequest, ChatStream, SdkError};

use super::accessors::AgentClientImpl;
use crate::adapters::input_buffer::{RuntimeInputEventDrainPort, RuntimeQueueDrainPort};

pub(super) async fn chat_impl(
    me: &AgentClientImpl,
    input: ChatRequest,
) -> Result<ChatStream, SdkError> {
    let queue_drain = input.queue_drain.clone();
    let input_events = input.input_events.clone();

    // 前置校验：如果初始输入包含图片但当前模型不支持
    if let Some(ref user_input) = input.user_input {
        if !user_input.images.is_empty() {
            let supports_image = me
                .inner
                .resolved_model
                .model
                .input
                .iter()
                .any(|t| t == "image");
            if !supports_image {
                let model_id = &me.inner.resolved_model.model.id;
                let provider = &me.inner.resolved_model.source_key;
                return Err(SdkError::Internal(format!(
                    "当前模型 {provider}/{model_id} 不支持图片输入。\
                     请切换到支持图片的模型（如 MiniMax/MiniMax-M3）或使用 /clear-images 清除待发送图片后重试。"
                )));
            }
        }
    }

    // #872: Runtime 不再持有/回写会话链；将初始 user_input 转为
    // Vec<Message> 并准备传 ChatLoopContext（历史由 Context backing 提供）。
    let initial_messages: Vec<share::message::Message> =
        if let Some(ref user_input) = input.user_input {
            let msg = if user_input.images.is_empty() {
                share::message::Message::user(&user_input.text)
            } else {
                share::message::Message::user_with_images(
                    user_input.text.clone(),
                    user_input
                        .images
                        .iter()
                        .map(|img| (img.id.clone(), img.base64.clone(), img.media_type.clone()))
                        .collect(),
                )
            };
            vec![msg]
        } else {
            Vec::new()
        };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = (me.inner.event_sink_factory)(tx);
    let inner = me.inner.clone();
    let session_context = logging::capture();
    logging::spawn_instrumented(session_context, async move {
        crate::application::main_loop::process_chat_loop(
            crate::application::main_loop::ChatLoopContext {
                sink,
                queue: RuntimeQueueDrainPort::new(queue_drain),
                input_events: RuntimeInputEventDrainPort::new(input_events),
                binding: inner.context.resources.binding.clone(),
                tool_catalog: inner.context.resources.tool_catalog.clone(),
                tool_execution: inner.context.resources.tool_execution.clone(),
                tool_context_binding: inner.context.resources.tool_context_binding.clone(),
                system_blocks: inner.context.resources.system_blocks.clone(),
                system_prompt_text: inner.context.resources.system_prompt_text.clone(),
                initial_git_context: inner.context.resources.initial_git_context.clone(),
                user_context: inner.context.resources.user_context.clone(),
                initial_messages,
                context_size: inner.context.resources.context_size,
                workspace: inner.workspace.clone(),
                wiring: inner.wiring.clone(),
                session_id: inner.session_id.clone(),
                read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
                session_reminders: Arc::new(Mutex::new(Default::default())),
                agent_runner: Some(inner.context.resources.agent_runner.clone()),
                tool_result_materializer: inner.context.resources.tool_result_materializer.clone(),
                policy: inner.context.resources.policy.clone(),
                active_run: inner.active_run.clone(),
                interaction_bridge: inner.interaction_bridge.clone(),
                task_access: inner.context.resources.task_access.clone(),
                max_tool_concurrency: inner.max_tool_concurrency,
                agent_semaphore: inner.context.resources.agent_semaphore.clone(),
                hook_runner: inner.context.resources.hook_runner.clone(),
                memory_config: inner.context.resources.memory_config.clone(),
                memory: inner.wiring.committed_memory(),
                reflection_history: inner.context.resources.reflection_history.clone(),
                language: inner.context.resources.language.clone(),
                reasoning: workflow::adaptive_reasoning(
                    inner.context.resources.binding.requested_reasoning,
                ),
                build_switched_client: {
                    let config_query = inner.config_query.clone();
                    let provider_factory = inner.context.resources.provider_factory.clone();
                    std::sync::Arc::new(move |selection: &str| {
                        let selection = selection.to_string();
                        let config_query = config_query.clone();
                        let provider_factory = provider_factory.clone();
                        Box::pin(async move {
                            super::trait_model::build_provider_binding_for_switch(
                                &selection,
                                config_query.as_ref(),
                                provider_factory.as_ref(),
                            )
                            .await
                        })
                    })
                },
                list_reflection_history: {
                    let inner = inner.clone();
                    std::sync::Arc::new(move |limit| {
                        let inner = inner.clone();
                        Box::pin(async move {
                            let me = super::accessors::AgentClientImpl { inner };
                            super::trait_reflection::list_reflection_history_impl(&me, limit).await
                        })
                    })
                },
                list_models: {
                    let inner = inner.clone();
                    std::sync::Arc::new(move || {
                        let inner = inner.clone();
                        Box::pin(async move {
                            let me = super::accessors::AgentClientImpl { inner };
                            super::trait_model::list_models_impl(&me).await
                        })
                    })
                },
                list_reminders: {
                    let inner = inner.clone();
                    std::sync::Arc::new(move || {
                        let inner = inner.clone();
                        Box::pin(async move {
                            let me = super::accessors::AgentClientImpl { inner };
                            super::trait_memory::list_reminders_impl(&me).await
                        })
                    })
                },
                list_sessions: {
                    let inner = inner.clone();
                    std::sync::Arc::new(move || {
                        let inner = inner.clone();
                        Box::pin(async move {
                            let me = super::accessors::AgentClientImpl { inner };
                            super::trait_session::list_sessions_impl(&me).await
                        })
                    })
                },
            },
        )
        .await;
        // #872: 不再回写 RuntimeHandle chain，不再 loop-exit auto-save。
        // session 持久化由 Context backing 统一负责。
    });

    Ok(ChatStream::new(rx))
}
