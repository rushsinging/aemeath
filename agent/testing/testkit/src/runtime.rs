//! runtime 测试替身（#2146 治理后统一由 testkit 维护）。
//!
//! ProviderFactory 实现权归 runtime crate，跨 crate 测试经本模块获得替身；
//! 消费方经 `[dev-dependencies] testkit` 引用，不进生产依赖图。
//!
//! 注意：runtime crate 自身 lib 测试专用的 model/reflection/tool fake 留在
//! runtime 内（`application/*/fakes.rs`）——Cargo dev-dependency 循环下对被测
//! crate 会产生双实例，`testkit::runtime` 仅供其它 crate 消费。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use provider::ProviderError;
use runtime::{ProviderBindingData, ProviderBuildSpecData, ProviderFactory};

/// 脚本化 ProviderFactory：build 按脚本产出 binding（可伪造任意端口/模型）。
pub fn scripted_provider_factory(
    script: impl Fn(ProviderBuildSpecData) -> Result<ProviderBindingData, ProviderError>
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
    F: Fn(ProviderBuildSpecData) -> Result<ProviderBindingData, ProviderError> + Send + Sync,
{
    fn build(&self, spec: ProviderBuildSpecData) -> Result<ProviderBindingData, ProviderError> {
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
