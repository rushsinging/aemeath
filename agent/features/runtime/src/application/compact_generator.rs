//! Compact LLM 绑定工厂（#1863 反转：context 直依赖 provider 通用端口
//! `LlmTextInvokePort`，本模块经 [`CompactModelResolver`] 解析模型并构造
//! [`CompactLlmBinding`]，由调用方按轮注入 compact 请求）。

use std::sync::Arc;

use crate::application::client::CompactModelResolver;
use crate::application::constants::COMPACT_UNKNOWN_MODEL_WINDOW;

/// compact 模型绑定工厂：包装 resolver，输出 context 请求可携带的
/// [`context::CompactLlmBinding`]。
pub struct CompactLlmBindingFactory {
    resolver: Arc<CompactModelResolver>,
    invoker: Arc<dyn provider::composition::LlmTextInvokePort>,
}

impl CompactLlmBindingFactory {
    pub fn new(
        resolver: Arc<CompactModelResolver>,
        invoker: Arc<dyn provider::composition::LlmTextInvokePort>,
    ) -> Self {
        Self { resolver, invoker }
    }

    /// 解析当前 compact 目标并构造 binding（None = 本地压缩）。
    pub fn resolve(&self) -> Option<context::CompactLlmBinding> {
        match self.resolver.resolve() {
            Ok(target) => {
                let binding = target.binding();
                let window = target.context_window().or_else(|| {
                    if matches!(
                        target.origin(),
                        crate::application::client::CompactModelOrigin::Configured
                    ) {
                        log::warn!(
                            target: crate::LOG_TARGET,
                            "[compact] compact 模型未声明输入窗口，改用保守窗口 {COMPACT_UNKNOWN_MODEL_WINDOW}"
                        );
                        Some(COMPACT_UNKNOWN_MODEL_WINDOW)
                    } else {
                        None
                    }
                });
                Some(context::CompactLlmBinding {
                    invoker: Arc::clone(&self.invoker) as _,
                    model: binding.model.clone(),
                    max_output_tokens: crate::application::constants::COMPACT_MAX_OUTPUT_TOKENS
                        .min(binding.max_tokens.max(1)),
                    compact_context_window: window,
                })
            }
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "[compact] 无法解析 compact 模型，本次走本地压缩：{error}"
                );
                None
            }
        }
    }
}
