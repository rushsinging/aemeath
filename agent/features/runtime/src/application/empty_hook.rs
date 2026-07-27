use async_trait::async_trait;
use hook::{HookDispatchContext, HookInvocation, HookOutcome, HookPort};
use tokio_util::sync::CancellationToken;

/// Empty Hook capability for Runs that must not execute any Hook.
#[derive(Debug, Clone, Default)]
pub struct EmptyHookPort;

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
