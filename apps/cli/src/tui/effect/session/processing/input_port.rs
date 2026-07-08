use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub(crate) struct TuiInputEventPort {
    rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<sdk::ChatInputEvent>>>,
}

impl TuiInputEventPort {
    pub(crate) fn channel() -> (mpsc::UnboundedSender<sdk::ChatInputEvent>, Self) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            tx,
            Self {
                rx: Arc::new(tokio::sync::Mutex::new(rx)),
            },
        )
    }
}

impl sdk::ChatInputEventPort for TuiInputEventPort {
    fn recv_next<'a>(&'a self) -> sdk::InputEventOptFuture<'a> {
        Box::pin(async move { self.rx.lock().await.recv().await })
    }

    fn drain_input_events<'a>(&'a self) -> sdk::InputEventFuture<'a> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        })
    }
}
