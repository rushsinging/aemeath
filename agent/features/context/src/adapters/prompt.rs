//! Prompt & Guidance 子模块（PromptPort）。
//!
//! 原 `agent/features/prompt/` crate 整体并入。

#[allow(dead_code, unused_imports)]
mod constants;
#[allow(dead_code, unused_imports)]
pub(crate) mod guidance;
pub(crate) mod security;
#[allow(dead_code)]
mod state;

#[cfg(test)]
pub(crate) use guidance::resolve_guidance;
pub use guidance::resolver::InstructionsLoadedHook;
pub use guidance::{init_guidance_dir, resolve_guidance_async};
// 仅 guidance 契约测试消费（prompt_source.rs 直连 share::i18n catalog）。
#[cfg(test)]
pub(crate) use guidance::universal_execution_discipline;
pub use security::assess_guidance;
