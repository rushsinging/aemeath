use async_trait::async_trait;
use hook::{
    CancellationSignal, HookClassData, HookDispatchContextData, HookDispatcher, HookInvocationData,
    HookOutcomeData,
};

#[derive(Clone)]
/// Sub Run 的 BoundaryOnly Hook adapter。
///
/// Hook-owned metadata 是过滤的唯一真相：Boundary point（含 Stop）转发到本 Run 的
/// frozen Dispatcher，Tool/Notification point 无副作用返回 Proceed。
pub struct BoundaryHookPort {
    inner: std::sync::Arc<dyn HookDispatcher>,
}

impl BoundaryHookPort {
    pub fn new(inner: std::sync::Arc<dyn HookDispatcher>) -> Self {
        Self { inner }
    }

    fn allows(point: hook::HookPointData) -> bool {
        point.metadata().class == HookClassData::Boundary
    }
}

#[async_trait]
impl HookDispatcher for BoundaryHookPort {
    async fn dispatch(
        &self,
        invocation: HookInvocationData,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcomeData {
        if Self::allows(invocation.point()) {
            self.inner.dispatch(invocation, cancellation).await
        } else {
            HookOutcomeData::proceed()
        }
    }

    async fn dispatch_at(
        &self,
        invocation: HookInvocationData,
        context: HookDispatchContextData,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcomeData {
        if Self::allows(invocation.point()) {
            self.inner
                .dispatch_at(invocation, context, cancellation)
                .await
        } else {
            HookOutcomeData::proceed()
        }
    }
}

#[cfg(test)]
#[path = "empty_tests.rs"]
mod tests;
