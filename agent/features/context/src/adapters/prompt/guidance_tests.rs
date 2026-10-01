use super::*;

static ENV_LOCK: &std::sync::Mutex<()> = &super::GUIDANCE_ENV_LOCK;

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
    let _guard = EnvVarGuard::set_path(paths::AGENTS_DIR_ENV, &temp_agents_dir);

    assert_eq!(guidance_dir(), Some(temp_agents_dir.join("guidance")));
}

#[test]
fn test_init_guidance_dir_creates_files() {
    let _lock = ENV_LOCK.lock().unwrap();
    let temp_agents_dir = unique_temp_dir("guidance_init");
    let guidance = temp_agents_dir.join("guidance");
    let _guard = EnvVarGuard::set_path(paths::AGENTS_DIR_ENV, &temp_agents_dir);
    let _ = std::fs::remove_dir_all(&temp_agents_dir);

    init_guidance_dir();

    // Check that empty placeholder files are created
    assert!(guidance.join("_default.md").exists());
    assert!(guidance.join("deepseek.md").exists());
    assert!(guidance.join("glm.md").exists());
    assert!(guidance.join("minimax.md").exists());
    assert!(guidance.join("_reasoning.md").exists());

    // Verify files are empty
    let content = std::fs::read_to_string(guidance.join("_default.md")).unwrap();
    assert!(content.is_empty());

    let content = std::fs::read_to_string(guidance.join("_reasoning.md")).unwrap();
    assert!(content.is_empty());

    let _ = std::fs::remove_dir_all(&temp_agents_dir);
}

#[test]
fn test_language_subdir_fallback() {
    let _lock = ENV_LOCK.lock().unwrap();
    let temp_agents_dir = unique_temp_dir("guidance_lang");
    let guidance = temp_agents_dir.join("guidance");
    let _guard = EnvVarGuard::set_path(paths::AGENTS_DIR_ENV, &temp_agents_dir);
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

    // Test fallback to built-in defaults when files are empty
    std::fs::write(guidance.join("_default.md"), "").unwrap();
    std::fs::write(zh_dir.join("_default.md"), "").unwrap();
    std::fs::write(en_dir.join("_default.md"), "").unwrap();

    // Should fallback to built-in default (English)
    let content = resolver::load_named_file_with_lang("_default", "en");
    assert!(content.is_some());
    assert!(content.unwrap().contains("English"));

    // Should fallback to built-in default (Chinese)
    let content = resolver::load_named_file_with_lang("_default", "zh");
    assert!(content.is_some());
    assert!(content.unwrap().contains("中文"));

    let _ = std::fs::remove_dir_all(&temp_agents_dir);
}
