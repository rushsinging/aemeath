//! chat() 方法实际逻辑。

use std::sync::{Arc, Mutex};

use sdk::{ChatRequest, ChatStream, SdkError};

use super::accessors::AgentClientImpl;
use crate::ports::{RuntimeInputEventDrainPort, RuntimeQueueDrainPort};

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

    // 获取 chain：优先复用 current_chain（resume 场景已 load），
    // 否则初始化空 chain（loop idle 会等第一条用户输入）。
    // 若有初始 user_input，构造首条消息放入 chain。
    let chain = {
        let existing = me
            .inner
            .current_chain
            .lock()
            .map_err(|_| SdkError::Internal("当前 session chain 锁已损坏".to_string()))?;
        if !existing.is_empty() {
            existing.clone()
        } else if let Some(ref user_input) = input.user_input {
            // 首次启动且有初始输入：构造单条消息
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
            context::session::ChatChain::from_flat_messages(vec![msg])
        } else {
            context::session::ChatChain::default()
        }
    };

    *me.inner
        .current_chain
        .lock()
        .map_err(|_| SdkError::Internal("当前 session chain 锁已损坏".to_string()))? =
        chain.clone();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = (me.inner.event_sink_factory)(tx);
    let inner = me.inner.clone();
    tokio::spawn(async move {
        let final_chain = crate::application::chat::process_chat_loop(
            crate::application::chat::ChatLoopContext {
                sink,
                queue: RuntimeQueueDrainPort::new(queue_drain),
                input_events: RuntimeInputEventDrainPort::new(input_events),
                client: inner.context.resources.client.clone(),
                registry: inner.context.resources.registry.clone(),
                system_blocks: inner.context.resources.system_blocks.clone(),
                system_prompt_text: inner.context.resources.system_prompt_text.clone(),
                user_context: inner.context.resources.user_context.clone(),
                chain,
                context_size: inner.context.resources.context_size,
                workspace: inner.workspace.clone(),
                session_id: inner.session_id.clone(),
                read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
                session_reminders: Arc::new(Mutex::new(Default::default())),
                agent_runner: Some(inner.context.resources.agent_runner.clone()),
                allow_all: inner.context.resources.allow_all,
                active_run: inner.active_run.clone(),
                task_store: inner.context.resources.task_store.clone(),
                max_tool_concurrency: inner.max_tool_concurrency,
                max_agent_concurrency: inner.max_agent_concurrency,
                agent_semaphore: inner.context.resources.agent_semaphore.clone(),
                hook_runner: inner.context.resources.hook_runner.clone(),
                memory_config: inner.context.resources.memory_config.clone(),
                language: inner.context.resources.language.clone(),
                frozen_chats: inner.frozen_chats.clone(),
                active_summary: inner.active_summary.clone(),
                reasoning_graph: inner
                    .context
                    .resources
                    .reasoning_graph_config
                    .as_ref()
                    .filter(|c| c.enabled)
                    .map(|c| crate::application::reasoning_graph::ReasoningGraph::new(c.clone())),
                build_switched_client: {
                    let cwd = inner.cwd.clone();
                    std::sync::Arc::new(move |selection: &str| {
                        let selection = selection.to_string();
                        let cwd = cwd.clone();
                        Box::pin(async move {
                            super::trait_model::build_llm_client_for_switch(&selection, &cwd).await
                        })
                    })
                },
                save_chain: {
                    let inner = inner.clone();
                    std::sync::Arc::new(move |chain: &context::session::ChatChain| {
                        let inner = inner.clone();
                        let chain = chain.clone();
                        Box::pin(async move {
                            super::trait_session::save_chain_to_handle(&chain, &inner).await
                        })
                    })
                },
                run_reflection_on_demand: {
                    let inner = inner.clone();
                    std::sync::Arc::new(move || {
                        let inner = inner.clone();
                        Box::pin(async move {
                            let me = super::accessors::AgentClientImpl { inner };
                            // 从 inner.current_chain 读取扁平消息
                            let messages = me.inner.current_chain.lock().unwrap().messages_flat();
                            let sdk_msgs: Vec<sdk::ChatMessage> = messages
                                .into_iter()
                                .map(super::mapping::message_to_sdk)
                                .collect();
                            super::trait_reflection::run_reflection_impl(&me, sdk_msgs).await
                        })
                    })
                },
                apply_reflection_on_demand: {
                    let inner = inner.clone();
                    std::sync::Arc::new(move |output: sdk::ReflectionOutputView| {
                        let inner = inner.clone();
                        Box::pin(async move {
                            let me = super::accessors::AgentClientImpl { inner };
                            super::trait_reflection::apply_reflection_impl(&me, output).await
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
        // 最终 chain 同步到共享 slot（覆盖 loop 内任何未同步的变更）。
        if let Ok(mut guard) = inner.current_chain.lock() {
            *guard = final_chain;
        }
        // auto-save：loop 退出后自动保存当前 session。TUI 退出时只需 drop input_event_tx →
        // loop shutdown → runtime 自动 save，不再调 session RPC。
        if let Err(e) = super::trait_session::save_session_from_handle(&inner).await {
            log::warn!(target: "aemeath:agent:runtime", "auto-save failed on loop exit: {e}");
        }
    });

    Ok(ChatStream::new(rx))
}
