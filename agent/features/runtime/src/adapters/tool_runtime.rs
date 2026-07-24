//! Tokio-specific tool runtime adapters.
//!
//! RuntimeWorkspaceAccess 类型在 application::workspace_access。
//! 本文件只保留 Tokio CancellationToken 和 mpsc channel 的 adapter 实现。

use async_trait::async_trait;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct TokioCancellation(pub CancellationToken);

#[async_trait]
impl tools::CancellationSignal for TokioCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
    async fn cancelled(&self) {
        self.0.cancelled().await
    }
    fn child_signal(&self) -> Arc<dyn tools::CancellationSignal> {
        Arc::new(Self(self.0.child_token()))
    }
}

pub fn cancellation(token: CancellationToken) -> Arc<dyn tools::CancellationSignal> {
    Arc::new(TokioCancellation(token))
}

pub struct ChannelProgress(pub tokio::sync::mpsc::Sender<tools::AgentProgressEvent>);

impl tools::ProgressSink for ChannelProgress {
    fn emit(&self, event: tools::AgentProgressEvent) {
        let _ = self.0.try_send(event);
    }
}

pub fn progress(
    tx: tokio::sync::mpsc::Sender<tools::AgentProgressEvent>,
) -> Arc<dyn tools::ProgressSink> {
    Arc::new(ChannelProgress(tx))
}
