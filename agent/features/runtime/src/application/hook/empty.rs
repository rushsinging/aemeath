use async_trait::async_trait;
use hook::{CancellationSignal, HookDispatchContext, HookInvocation, HookOutcome, HookPort};

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
        cancellation: &dyn CancellationSignal,
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
        cancellation: &dyn CancellationSignal,
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
