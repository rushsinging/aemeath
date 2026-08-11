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
