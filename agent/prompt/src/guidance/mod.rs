//! Model guidance resolution logic.
//!
//! Guidance files are loaded from `~/.agents/guidance/` directory:
//!   - `_default.md`          — injected for ALL models
//!   - `{prefix}.md`          — matched by model-id prefix (longest match wins)
//!                               e.g. `glm.md` matches `glm-5.1`, `deepseek.md` matches `deepseek-chat`
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

use std::path::PathBuf;

pub mod constants;
pub mod resolver;

const AGENTS_DIR_ENV: &str = "AEMEATH_AGENTS_DIR";

fn global_agents_dir() -> PathBuf {
    std::env::var_os(AGENTS_DIR_ENV)
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".agents")))
        .unwrap_or_else(|| PathBuf::from(".agents"))
}

fn global_guidance_dir() -> PathBuf {
    global_agents_dir().join("guidance")
}

// Re-export public API so external code can use `share::guidance::...` unchanged.
pub use constants::UNIVERSAL_EXECUTION_DISCIPLINE;
pub use resolver::{
    load_named_file_async, resolve_guidance, resolve_guidance_async, resolve_model_guidance_async,
};

/// Returns the default guidance dir: `~/.agents/guidance/`
pub fn guidance_dir() -> Option<PathBuf> {
    Some(global_guidance_dir())
}

/// Initialise the guidance directory with default files.
///
/// Creates the directory if missing, then writes any default files that
/// don't yet exist. Existing files are **never** overwritten.
pub fn init_guidance_dir() {
    let dir = match guidance_dir() {
        Some(d) => d,
        None => return,
    };

    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("Failed to create guidance dir {}: {}", dir.display(), e);
            return;
        }
    }

    for (filename, content) in constants::DEFAULT_FILES {
        let path = dir.join(filename);
        if path.exists() {
            continue; // never overwrite user-edited files
        }
        if let Err(e) = std::fs::write(&path, content.trim()) {
            log::warn!("Failed to write {}: {}", path.display(), e);
        }
    }

    log::info!("Initialised default guidance files in {}", dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guidance_dir_uses_agents_directory() {
        let temp_agents_dir = std::env::temp_dir().join(format!(
            "aemeath_guidance_dir_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let previous = std::env::var_os(AGENTS_DIR_ENV);
        std::env::set_var(AGENTS_DIR_ENV, &temp_agents_dir);

        assert_eq!(guidance_dir(), Some(temp_agents_dir.join("guidance")));

        if let Some(previous) = previous {
            std::env::set_var(AGENTS_DIR_ENV, previous);
        } else {
            std::env::remove_var(AGENTS_DIR_ENV);
        }
    }

    #[test]
    fn test_init_guidance_dir_creates_files() {
        let tmp = std::env::temp_dir().join("aemeath_test_guidance_init");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        for (filename, content) in constants::DEFAULT_FILES {
            std::fs::write(tmp.join(filename), content.trim()).unwrap();
        }

        assert!(tmp.join("_default.md").exists());
        assert!(tmp.join("glm.md").exists());
        assert!(tmp.join("deepseek.md").exists());
        assert!(tmp.join("_reasoning.md").exists());

        let content = std::fs::read_to_string(tmp.join("_reasoning.md")).unwrap();
        assert!(content.contains("think/reason in Chinese"));

        let default = std::fs::read_to_string(tmp.join("_default.md")).unwrap();
        assert!(default.contains("strictly valid JSON"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
