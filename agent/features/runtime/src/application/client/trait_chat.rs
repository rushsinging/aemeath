//! chat() 方法实际逻辑。

use std::sync::{Arc, Mutex};

use sdk::{ChatRequest, ChatStream, SdkError};

use super::accessors::AgentClientImpl;
use super::session_query::AgentSessionQuery;

pub(super) async fn chat_impl(
    me: &AgentClientImpl,
    input: ChatRequest,
) -> Result<ChatStream, SdkError> {
    let input_events = (me.inner.shell.input_port_factory)(input.ingress);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    // #252 PR3：spinner 活动数事件直达本 chat 会话通道（覆盖式刷新；
    // 后台账本事件无需经 sink 工厂重建孤立通道）。
    me.inner
        .shell
        .background_tasks
        .bind_chat_event_sender(tx.clone());
    let sink = (me.inner.shell.event_sink_factory)(tx);
    let shell = me.inner.shell.clone();
    let inner = me.inner.clone();
    let session_context = logging::capture();
    logging::spawn_instrumented(session_context, async move {
        crate::application::loop_engine::chat::run_session_command_driver(
            crate::application::loop_engine::chat::SessionCommandDriverInput {
                sink,
                input_events,
                session: shell.clone(),
                read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
                session_queries: Arc::new(AgentSessionQuery::new(Arc::new(AgentClientImpl {
                    inner: inner.clone(),
                }))),
                background_wakeup: shell.take_background_wakeup_waiter(),
            },
        )
        .await;
        // #872: 不再回写 RuntimeHandle chain，不再 loop-exit auto-save。
        // session 持久化由 Context backing 统一负责。
    });

    Ok(ChatStream::new(rx))
}
