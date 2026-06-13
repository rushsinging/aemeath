//! Model guidance resolution logic.
//!
//! Guidance files are loaded from `~/.agents/guidance/` directory:
//!   - `_default.md`          — injected for ALL models
//!   - `{prefix}.md`          — matched by model-id prefix (longest match wins)
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
pub use resolver::{resolve_guidance, resolve_guidance_async, resolve_model_guidance_async};

/// Returns the default guidance dir: `~/.agents/guidance/`
pub fn guidance_dir() -> Option<PathBuf> {
    Some(global_guidance_dir())
}

/// Initialise the guidance directory with default files.
///
/// Creates the directory structure:
///   ~/.agents/guidance/
///   ├── en/
///   │   └── (English guidance files)
///   └── zh/
///       └── (Chinese guidance files)
///
/// Existing files are **never** overwritten.
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

    // Initialize language subdirectories
    for (lang, files) in constants::SUPPORTED_LANGUAGES {
        let lang_dir = dir.join(lang);
        if !lang_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&lang_dir) {
                log::warn!("Failed to create guidance lang dir {}: {}", lang_dir.display(), e);
                continue;
            }
        }

        for (filename, content) in *files {
            let path = lang_dir.join(filename);
            if path.exists() {
                continue; // never overwrite user-edited files
            }
            if let Err(e) = std::fs::write(&path, content.trim()) {
                log::warn!("Failed to write {}: {}", path.display(), e);
            }
        }
    }

    log::info!("Initialised default guidance files in {}", dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aemeath_{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn test_guidance_dir_uses_agents_directory() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp_agents_dir = unique_temp_dir("guidance_dir");
        let _guard = EnvVarGuard::set_path(AGENTS_DIR_ENV, &temp_agents_dir);

        assert_eq!(guidance_dir(), Some(temp_agents_dir.join("guidance")));
    }

    #[test]
    fn test_init_guidance_dir_creates_files() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp_agents_dir = unique_temp_dir("guidance_init");
        let guidance = temp_agents_dir.join("guidance");
        let _guard = EnvVarGuard::set_path(AGENTS_DIR_ENV, &temp_agents_dir);
        let _ = std::fs::remove_dir_all(&temp_agents_dir);

        init_guidance_dir();

        // Check English subdirectory
        assert!(guidance.join("en/_default.md").exists());
        assert!(guidance.join("en/glm.md").exists());
        assert!(guidance.join("en/deepseek.md").exists());
        assert!(guidance.join("en/_reasoning.md").exists());

        // Check Chinese subdirectory
        assert!(guidance.join("zh/_default.md").exists());
        assert!(guidance.join("zh/glm.md").exists());
        assert!(guidance.join("zh/deepseek.md").exists());
        assert!(guidance.join("zh/_reasoning.md").exists());

        // Verify content
        let content = std::fs::read_to_string(guidance.join("en/_reasoning.md")).unwrap();
        assert!(content.contains("think/reason in English"));

        let content = std::fs::read_to_string(guidance.join("zh/_reasoning.md")).unwrap();
        assert!(content.contains("中文"));

        let _ = std::fs::remove_dir_all(&temp_agents_dir);
    }

    #[test]
    fn test_language_subdir_fallback() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp_agents_dir = unique_temp_dir("guidance_lang");
        let guidance = temp_agents_dir.join("guidance");
        let _guard = EnvVarGuard::set_path(AGENTS_DIR_ENV, &temp_agents_dir);
        let _ = std::fs::remove_dir_all(&temp_agents_dir);

        // Create root file only (no language subdirectory)
        std::fs::create_dir_all(&guidance).unwrap();
        std::fs::write(guidance.join("_default.md"), "root content").unwrap();

        // With language="zh", should fallback to root file
        let content = resolver::load_named_file_with_lang("_default", "zh");
        assert_eq!(content, Some("root content".to_string()));

        // Create Chinese subdirectory with file
        let zh_dir = guidance.join("zh");
        std::fs::create_dir_all(&zh_dir).unwrap();
        std::fs::write(zh_dir.join("_default.md"), "zh content").unwrap();

        // Now should prefer Chinese version
        let content = resolver::load_named_file_with_lang("_default", "zh");
        assert_eq!(content, Some("zh content".to_string()));

        // English should still use root (no en/ directory)
        let content = resolver::load_named_file_with_lang("_default", "en");
        assert_eq!(content, Some("root content".to_string()));

        // Create English subdirectory
        let en_dir = guidance.join("en");
        std::fs::create_dir_all(&en_dir).unwrap();
        std::fs::write(en_dir.join("_default.md"), "en content").unwrap();

        // Now English should use its own
        let content = resolver::load_named_file_with_lang("_default", "en");
        assert_eq!(content, Some("en content".to_string()));

        let _ = std::fs::remove_dir_all(&temp_agents_dir);
    }
}
