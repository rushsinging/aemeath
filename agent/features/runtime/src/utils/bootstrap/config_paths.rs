use share::config::paths;
use std::path::{Path, PathBuf};

pub fn global_agents_dir() -> PathBuf {
    if let Ok(value) = std::env::var(paths::AGENTS_DIR_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return expand_home(Path::new(trimmed));
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(paths::AGENTS_DIR_NAME)
}

pub fn global_config_path() -> PathBuf {
    global_agents_dir().join(paths::NEW_CONFIG_FILE)
}

pub fn old_global_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(paths::OLD_AEMEATH_DIR_NAME)
        .join(paths::OLD_CONFIG_FILE)
}

pub fn global_agents_md_path() -> PathBuf {
    global_agents_dir().join(paths::AGENTS_MD)
}

pub fn old_global_claude_md_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(paths::CLAUDE_DIR_NAME)
        .join(paths::CLAUDE_MD)
}

pub fn global_skills_dir() -> PathBuf {
    global_agents_dir().join(paths::SKILLS_DIR_NAME)
}

pub fn global_logs_dir() -> PathBuf {
    global_agents_dir().join(paths::LOGS_DIR_NAME)
}

pub fn global_guidance_dir() -> PathBuf {
    global_agents_dir().join(paths::GUIDANCE_DIR_NAME)
}

pub fn global_memory_dir() -> PathBuf {
    global_agents_dir().join(paths::MEMORY_DIR_NAME)
}

pub fn global_sessions_dir() -> PathBuf {
    global_agents_dir().join(paths::SESSIONS_DIR_NAME)
}

pub fn global_hooks_dir() -> PathBuf {
    global_agents_dir().join(paths::HOOKS_DIR_NAME)
}

pub fn global_mcp_config_path() -> PathBuf {
    global_agents_dir().join(paths::MCP_CONFIG_FILE)
}

pub fn global_history_path() -> PathBuf {
    global_agents_dir().join(paths::HISTORY_FILE)
}

pub fn global_cost_history_path() -> PathBuf {
    global_agents_dir().join(paths::COST_HISTORY_FILE)
}

pub fn global_settings_path() -> PathBuf {
    global_agents_dir().join(paths::SETTINGS_FILE)
}

pub fn old_global_skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(paths::OLD_AEMEATH_DIR_NAME)
        .join(paths::SKILLS_DIR_NAME)
}

pub async fn migrate_file_once(old_path: &Path, new_path: &Path) -> Result<bool, String> {
    if new_path.exists() || !old_path.exists() {
        return Ok(false);
    }

    if let Some(parent) = new_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建迁移目标目录失败 {}: {e}", parent.display()))?;
    }

    tokio::fs::copy(old_path, new_path).await.map_err(|e| {
        format!(
            "迁移文件失败 {} -> {}: {e}",
            old_path.display(),
            new_path.display()
        )
    })?;

    Ok(true)
}

pub fn migrate_dir_once(old_path: &Path, new_path: &Path) -> Result<bool, String> {
    if new_path.exists() || !old_path.exists() {
        return Ok(false);
    }
    if !old_path.is_dir() {
        return Ok(false);
    }

    copy_dir_all(old_path, new_path).map_err(|e| {
        format!(
            "迁移目录失败 {} -> {}: {e}",
            old_path.display(),
            new_path.display()
        )
    })?;
    Ok(true)
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else if ty.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn expand_home(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

pub async fn ensure_agents_dirs() -> Result<(), String> {
    for dir in [
        global_agents_dir(),
        global_skills_dir(),
        global_logs_dir(),
        global_guidance_dir(),
        global_memory_dir(),
        global_sessions_dir(),
        global_hooks_dir(),
    ] {
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| format!("创建目录失败 {}: {e}", dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) struct TestEnvGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl TestEnvGuard {
    pub(crate) fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let guard = TEST_ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
        let old = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key,
            old,
            _guard: guard,
        }
    }
}

#[cfg(test)]
impl Drop for TestEnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_global_logs_dir_uses_agents_logs_directory() {
        let temp_agents_dir = std::env::temp_dir().join(format!(
            "aemeath_agents_logs_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _guard = TestEnvGuard::set(paths::AGENTS_DIR_ENV, &temp_agents_dir);

        assert_eq!(global_logs_dir(), temp_agents_dir.join("logs"));
    }

    #[test]
    fn test_global_data_paths_use_agents_directory() {
        let temp_agents_dir = std::env::temp_dir().join(format!(
            "aemeath_agents_data_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _guard = TestEnvGuard::set(paths::AGENTS_DIR_ENV, &temp_agents_dir);

        assert_eq!(global_guidance_dir(), temp_agents_dir.join("guidance"));
        assert_eq!(global_memory_dir(), temp_agents_dir.join("memory"));
        assert_eq!(global_sessions_dir(), temp_agents_dir.join("sessions"));
        assert_eq!(global_hooks_dir(), temp_agents_dir.join("hooks"));
        assert_eq!(global_mcp_config_path(), temp_agents_dir.join("mcp.json"));
        assert_eq!(global_history_path(), temp_agents_dir.join("history.json"));
        assert_eq!(
            global_cost_history_path(),
            temp_agents_dir.join("cost_history.json")
        );
        assert_eq!(
            global_settings_path(),
            temp_agents_dir.join("settings.json")
        );
    }

    #[test]
    fn test_migrate_dir_once_copies_nested_files_without_overwrite() {
        let base = std::env::temp_dir().join(format!(
            "aemeath_migrate_dir_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old = base.join("old");
        let new = base.join("new");
        std::fs::create_dir_all(old.join("nested")).unwrap();
        let mut file = std::fs::File::create(old.join("nested").join("SKILL.md")).unwrap();
        write!(file, "skill").unwrap();

        assert!(migrate_dir_once(&old, &new).unwrap());
        assert_eq!(
            std::fs::read_to_string(new.join("nested").join("SKILL.md")).unwrap(),
            "skill"
        );
        assert!(!migrate_dir_once(&old, &new).unwrap());

        std::fs::remove_dir_all(base).unwrap();
    }
}
