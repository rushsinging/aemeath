use async_trait::async_trait;
use hook::{HookDispatchContext, HookInvocation, HookOutcome, HookPort};
use tokio_util::sync::CancellationToken;

/// Empty Hook capability for Runs that must not execute any Hook.
#[derive(Debug, Clone, Default)]
pub struct EmptyHookPort;

#[derive(Clone)]
pub struct BoundaryHookPort {
    inner: std::sync::Arc<dyn HookPort>,
}

impl BoundaryHookPort {
    pub fn new(inner: std::sync::Arc<dyn HookPort>) -> Self {
        Self { inner }
    }

    fn allows(invocation: &HookInvocation) -> bool {
        matches!(
            invocation,
            HookInvocation::SessionStart(_)
                | HookInvocation::SessionEnd(_)
                | HookInvocation::SubRunStart(_)
                | HookInvocation::SubRunStop(_)
        )
    }
}

#[async_trait]
impl HookPort for BoundaryHookPort {
    async fn dispatch(
        &self,
        invocation: HookInvocation,
        cancellation: &CancellationToken,
    ) -> HookOutcome {
        if Self::allows(&invocation) {
            self.inner.dispatch(invocation, cancellation).await
        } else {
            HookOutcome::proceed()
        }
    }

    async fn dispatch_at(
        &self,
        invocation: HookInvocation,
        context: HookDispatchContext,
        cancellation: &CancellationToken,
    ) -> HookOutcome {
        if Self::allows(&invocation) {
            self.inner
                .dispatch_at(invocation, context, cancellation)
                .await
        } else {
            HookOutcome::proceed()
        }
    }
}

#[async_trait]
impl HookPort for EmptyHookPort {
    async fn dispatch(
        &self,
        _invocation: HookInvocation,
        _cancellation: &CancellationToken,
    ) -> HookOutcome {
        HookOutcome::proceed()
    }

    async fn dispatch_at(
        &self,
        _invocation: HookInvocation,
        _context: HookDispatchContext,
        _cancellation: &CancellationToken,
    ) -> HookOutcome {
        HookOutcome::proceed()
    }
}
