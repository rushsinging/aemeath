//! chat() 方法实际逻辑。

use std::sync::{Arc, Mutex};

use sdk::{ChatRequest, ChatStream, SdkError};

use super::accessors::AgentClientImpl;
use super::session_query::AgentSessionQuery;

pub(super) async fn chat_impl(
    me: &AgentClientImpl,
    input: ChatRequest,
) -> Result<ChatStream, SdkError> {
    let queue_drain = input.queue_drain.clone();
    let input_events = input.input_events.clone();

    // #872: Runtime 不再持有/回写会话链；将初始 user_input 转为
    // Vec<Message> 并准备传 ChatLoopContext（历史由 Context backing 提供）。
    let initial_messages = Vec::new();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = (me.inner.shell.event_sink_factory)(tx);
    let input_ports = (me.inner.shell.input_port_factory)(queue_drain, input_events);
    let shell = me.inner.shell.clone();
    let inner = me.inner.clone();
    let session_context = logging::capture();
    logging::spawn_instrumented(session_context, async move {
        crate::application::loop_engine::chat::process_chat_loop(
            crate::application::loop_engine::chat::ChatLoopContext {
                sink,
                queue: input_ports.queue,
                input_events: input_ports.input_events,
                shell: shell.clone(),
                initial_messages,
                read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
                // Task 7 debt: ChatLoopContext uses tools::SessionReminders type;
                // shell holds share::memory::SessionReminders (different type).
                session_reminders: Arc::new(Mutex::new(Default::default())),
                session_queries: Arc::new(AgentSessionQuery::new(Arc::new(AgentClientImpl {
                    inner: inner.clone(),
                }))),
            },
        )
        .await;
        // #872: 不再回写 RuntimeHandle chain，不再 loop-exit auto-save。
        // session 持久化由 Context backing 统一负责。
    });

    Ok(ChatStream::new(rx))
}
