//! chat() 方法实际逻辑。

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use sdk::{ChatRequest, ChatStream, SdkError};

use super::accessors::AgentClientImpl;
use super::event::{RuntimeQueueDrainPort, SdkChatEventSink};
use super::mapping::message_from_sdk;

pub(super) async fn chat_impl(
    me: &AgentClientImpl,
    input: ChatRequest,
) -> Result<ChatStream, SdkError> {
    me.inner.cancel_token.store(false, Ordering::Release);
    let cancel = tokio_util::sync::CancellationToken::new();
    *me.inner
        .current_cancel
        .lock()
        .map_err(|_| SdkError::Internal("当前 chat 取消锁已损坏".to_string()))? =
        Some(cancel.clone());
    let queue_drain = input.queue_drain.clone();
    let messages: Vec<_> = input.messages.into_iter().map(message_from_sdk).collect();
    *me.inner
        .current_messages
        .lock()
        .map_err(|_| SdkError::Internal("当前 session 消息锁已损坏".to_string()))? =
        messages.clone();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = SdkChatEventSink {
        tx,
        current_messages: me.inner.current_messages.clone(),
        workspace_context: me.inner.workspace_context.clone(),
        change_tx: me.inner.change_tx.clone(),
    };
    let inner = me.inner.clone();
    tokio::spawn(async move {
        crate::business::chat::process_chat_loop(crate::business::chat::ChatLoopContext {
            sink,
            queue: RuntimeQueueDrainPort::new(queue_drain),
            client: inner.context.client.clone(),
            registry: inner.context.registry.clone(),
            system_blocks: inner.context.system_blocks.clone(),
            system_prompt_text: inner.context.system_prompt_text.clone(),
            user_context: inner.context.user_context.clone(),
            messages,
            context_size: inner.context.context_size,
            cwd: inner.cwd.clone(),
            workspace_context: inner.workspace_context.lock().ok().and_then(|g| g.clone()),
            session_id: inner.session_id.clone(),
            read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
            session_reminders: Arc::new(Mutex::new(Default::default())),
            agent_runner: Some(inner.context.agent_runner.clone()),
            allow_all: inner.context.allow_all,
            interrupted: inner.cancel_token.clone(),
            cancel,
            task_store: inner.context.task_store.clone(),
            max_tool_concurrency: inner.max_tool_concurrency,
            max_agent_concurrency: inner.max_agent_concurrency,
            agent_semaphore: inner.context.agent_semaphore.clone(),
            change_notifier: Some(share::tool::ToolChangeNotifier::new(
                inner.tool_change_tx.clone(),
            )),
            hook_runner: inner.context.hook_runner.clone(),
            memory_config: inner.context.memory_config.clone(),
            json_logger: inner.context.json_logger.clone(),
        })
        .await;
        if let Ok(mut guard) = inner.current_cancel.lock() {
            *guard = None;
        }
    });
    Ok(ChatStream::new(rx))
}
