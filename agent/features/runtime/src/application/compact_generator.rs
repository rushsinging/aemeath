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
                let max_output_tokens = crate::application::constants::COMPACT_MAX_OUTPUT_TOKENS
                    .min(binding.max_tokens.max(1));
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
                // 压缩耗时与模型强相关：解析结果（含来源与窗口）必须先落日志，
                // 否则慢的时候无法判断用的是配置模型还是会话模型。
                log::info!(
                    target: crate::LOG_TARGET,
                    "[compact] 目标模型解析：{}/{} origin={:?} compact_window={window:?} max_output={max_output_tokens}",
                    binding.model.provider,
                    binding.model.model,
                    target.origin(),
                );
                Some(context::CompactLlmBinding {
                    invoker: Arc::clone(&self.invoker) as _,
                    model: binding.model.clone(),
                    max_output_tokens,
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
