use async_trait::async_trait;
use hook::{
    CancellationSignal, HookClass, HookDispatchContext, HookInvocation, HookOutcome, HookPort,
};

#[derive(Clone)]
/// Sub Run 的 BoundaryOnly Hook adapter。
///
/// Hook-owned metadata 是过滤的唯一真相：Boundary point（含 Stop）转发到本 Run 的
/// frozen Dispatcher，Tool/Notification point 无副作用返回 Proceed。
pub struct BoundaryHookPort {
    inner: std::sync::Arc<dyn HookPort>,
}

impl BoundaryHookPort {
    pub fn new(inner: std::sync::Arc<dyn HookPort>) -> Self {
        Self { inner }
    }

    fn allows(point: hook::HookPoint) -> bool {
        point.metadata().class == HookClass::Boundary
    }
}

#[async_trait]
impl HookPort for BoundaryHookPort {
    async fn dispatch(
        &self,
        invocation: HookInvocation,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcome {
        if Self::allows(invocation.point()) {
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
        if Self::allows(invocation.point()) {
            self.inner
                .dispatch_at(invocation, context, cancellation)
                .await
        } else {
            HookOutcome::proceed()
        }
    }
}

#[cfg(test)]
#[path = "empty_tests.rs"]
mod tests;
