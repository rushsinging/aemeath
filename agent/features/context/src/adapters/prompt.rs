//! Prompt & Guidance 子模块（PromptPort）。
//!
//! 原 `agent/features/prompt/` crate 整体并入。

#[allow(dead_code, unused_imports)]
pub(crate) mod guidance;
pub(crate) mod security;

pub use guidance::resolver::InstructionsLoadedHook;
pub use guidance::{init_guidance_dir, resolve_guidance_async};
#[cfg(any(test, feature = "dev"))]
pub(crate) use guidance::{resolve_guidance, universal_execution_discipline};
pub use security::assess_guidance;
