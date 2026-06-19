//! chat() 方法实际逻辑。

use std::sync::{Arc, Mutex};

use sdk::{ChatRequest, ChatStream, SdkError};

use super::accessors::AgentClientImpl;
use super::event::{RuntimeInputEventDrainPort, RuntimeQueueDrainPort, SdkChatEventSink};
use super::mapping::message_from_sdk;

pub(super) async fn chat_impl(
    me: &AgentClientImpl,
    input: ChatRequest,
) -> Result<ChatStream, SdkError> {
    let cancel = tokio_util::sync::CancellationToken::new();
    *me.inner
        .current_cancel
        .lock()
        .map_err(|_| SdkError::Internal("当前 chat 取消锁已损坏".to_string()))? =
        Some(cancel.clone());
    let queue_drain = input.queue_drain.clone();
    let input_events = input.input_events.clone();
    let messages: Vec<_> = input.messages.into_iter().map(message_from_sdk).collect();

    // 前置校验：如果消息包含图片但当前模型不支持图片输入，返回清晰错误
    let has_image = messages.iter().any(|msg| {
        msg.content
            .iter()
            .any(|block| matches!(block, share::message::ContentBlock::Image { .. }))
    });
    if has_image {
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

    *me.inner
        .current_messages
        .lock()
        .map_err(|_| SdkError::Internal("当前 session 消息锁已损坏".to_string()))? =
        messages.clone();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = SdkChatEventSink {
        tx,
        current_messages: me.inner.current_messages.clone(),
        change_tx: me.inner.change_tx.clone(),
    };
    let inner = me.inner.clone();
    tokio::spawn(async move {
        crate::business::chat::process_chat_loop(crate::business::chat::ChatLoopContext {
            sink,
            queue: RuntimeQueueDrainPort::new(queue_drain),
            input_events: RuntimeInputEventDrainPort::new(input_events),
            client: inner.context.client.clone(),
            registry: inner.context.registry.clone(),
            system_blocks: inner.context.system_blocks.clone(),
            system_prompt_text: inner.context.system_prompt_text.clone(),
            user_context: inner.context.user_context.clone(),
            messages,
            context_size: inner.context.context_size,
            cwd: inner.cwd.clone(),
            workspace: inner.workspace.clone(),
            session_id: inner.session_id.clone(),
            read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
            session_reminders: Arc::new(Mutex::new(Default::default())),
            agent_runner: Some(inner.context.agent_runner.clone()),
            allow_all: inner.context.allow_all,
            cancel,
            task_store: inner.context.task_store.clone(),
            max_tool_concurrency: inner.max_tool_concurrency,
            max_agent_concurrency: inner.max_agent_concurrency,
            agent_semaphore: inner.context.agent_semaphore.clone(),
            hook_runner: inner.context.hook_runner.clone(),
            memory_config: inner.context.memory_config.clone(),
            language: inner.context.language.clone(),
            frozen_chats: inner.frozen_chats.clone(),
            active_summary: inner.active_summary.clone(),
        })
        .await;
        if let Ok(mut guard) = inner.current_cancel.lock() {
            *guard = None;
        }
    });
    Ok(ChatStream::new(rx))
}
