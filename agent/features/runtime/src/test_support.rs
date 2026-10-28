//! Runtime 测试实现面（#2146 实现权治理：ProviderFactory 实现权归本
//! crate，跨 crate 测试经此获得替身；生产不编译）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::ports::{ProviderBindingData, ProviderBuildSpecData, ProviderFactory};

/// 脚本化 ProviderFactory：build 按脚本产出 binding（可伪造任意端口/模型）。
pub fn scripted_provider_factory(
    script: impl Fn(ProviderBuildSpecData) -> Result<ProviderBindingData, provider::ProviderError>
        + Send
        + Sync
        + 'static,
) -> Arc<dyn ProviderFactory> {
    Arc::new(ScriptedFactory {
        script: Arc::new(script),
    })
}

struct ScriptedFactory<F> {
    script: Arc<F>,
}

impl<F> ProviderFactory for ScriptedFactory<F>
where
    F: Fn(ProviderBuildSpecData) -> Result<ProviderBindingData, provider::ProviderError>
        + Send
        + Sync,
{
    fn build(
        &self,
        spec: ProviderBuildSpecData,
    ) -> Result<ProviderBindingData, provider::ProviderError> {
        (self.script)(spec)
    }
}

/// 计数 + 委托内层 factory：返回（factory 句柄, 计数器共享）。
pub fn counting_provider_factory(
    inner: Arc<dyn ProviderFactory>,
) -> (Arc<dyn ProviderFactory>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let factory = scripted_provider_factory(move |spec| {
        calls.fetch_add(1, Ordering::SeqCst);
        inner.build(spec)
    });
    (factory, counter)
}
