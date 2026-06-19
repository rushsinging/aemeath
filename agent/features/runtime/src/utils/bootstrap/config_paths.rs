use share::config::paths;
use std::path::Path;

// 全局路径函数统一转发到 `share::config::paths`（单一可变状态源），
// 本文件仅保留 runtime 特有的迁移 / 目录初始化逻辑与测试工具。
pub use paths::{
    expand_home, global_agents_dir, global_agents_md_path, global_config_path,
    global_cost_history_path, global_guidance_dir, global_history_path, global_hooks_dir,
    global_logs_dir, global_mcp_config_path, global_memory_dir, global_sessions_dir,
    global_settings_path, global_skills_dir, old_global_claude_md_path, old_global_config_path,
    old_global_skills_dir,
};

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
