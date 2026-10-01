//! Model guidance resolution logic.
//!
//! Guidance files are loaded from `~/.agents/guidance/` directory:
//!   - `_default.md`          — injected for ALL models
//!   - `{prefix}.md`          — all matching model-id prefixes, general to specific
//!     e.g. `glm.md` matches `glm-5.1`, `deepseek.md` matches `deepseek-chat`
//!   - `_reasoning.md`        — appended when reasoning/thinking is enabled
//!
//! Prefix matching is case-insensitive: `glm.md` matches `GLM-5.1`.
//!
//! On first run, default guidance files are auto-generated so users can edit them.
//! Guidance content lives entirely in the md files — this module only handles loading logic.
//!
//! **NOTE**: 不要在 DEFAULT_GUIDANCE 中硬编码具体的行为要求（如推理长度限制、语言偏好等）。
//! 这些内容应该由用户在 `~/.agents/guidance/` 下的 md 文件中自行配置。
//! 此处仅提供最小可用的初始模板，让用户知道文件格式和可用选项。

#[cfg(test)]
use super::state::GUIDANCE_ENV_LOCK;
use share::config::paths;
use std::path::PathBuf;

pub mod constants;
pub mod resolver;

fn global_guidance_dir() -> PathBuf {
    paths::global_guidance_dir()
}

// Re-export public API so external code can use `share::guidance::...` unchanged.
//
// 注：universal_execution_discipline 已迁至项目级 i18n catalog
// （share::i18n::prompt::discipline）。此处 re-export 保持调用点零改动。
pub use constants::{DEFAULT_FILES_EN, DEFAULT_FILES_ZH, DEFAULT_FILE_NAMES, SUPPORTED_LANGUAGES};
pub(crate) use resolver::resolve_guidance;
pub use resolver::{resolve_guidance_async, resolve_model_guidance_async};
pub(crate) use share::i18n::prompt::discipline::universal_execution_discipline;
pub use share::i18n::prompt::discipline::{
    UNIVERSAL_EXECUTION_DISCIPLINE_EN, UNIVERSAL_EXECUTION_DISCIPLINE_ZH,
};

/// Returns the default guidance dir: `~/.agents/guidance/`
pub fn guidance_dir() -> Option<PathBuf> {
    Some(global_guidance_dir())
}

/// Initialise the guidance directory with empty placeholder files.
///
/// Creates empty files in `~/.agents/guidance/`:
///   - `_default.md`
///   - `deepseek.md`
///   - `glm.md`
///   - `minimax.md`
///   - `_reasoning.md`
///
/// Users fill in their own content. Built-in defaults are used as fallback.
pub fn init_guidance_dir() {
    let dir = match guidance_dir() {
        Some(d) => d,
        None => return,
    };

    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!(target: crate::LOG_TARGET, "Failed to create guidance dir {}: {}", dir.display(), e);
            return;
        }
    }

    // Create empty placeholder files
    for &filename in constants::DEFAULT_FILE_NAMES {
        let path = dir.join(filename);
        if path.exists() {
            continue; // never overwrite user-edited files
        }
        if let Err(e) = std::fs::File::create(&path) {
            log::warn!(target: crate::LOG_TARGET, "Failed to create {}: {}", path.display(), e);
        }
    }

    log::info!(target: crate::LOG_TARGET, "Initialised guidance files in {}", dir.display());
}

#[cfg(test)]
#[path = "guidance_tests.rs"]
mod tests;
