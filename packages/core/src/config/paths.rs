//! Codex 风格配置路径。

use std::path::{Path, PathBuf};

pub const AGENTS_DIR_ENV: &str = "AEMEATH_AGENTS_DIR";
pub const NEW_CONFIG_FILE: &str = "aemeath.json";
pub const OLD_CONFIG_FILE: &str = "config.json";
pub const AGENTS_MD: &str = "AGENTS.md";
pub const CLAUDE_MD: &str = "CLAUDE.md";
pub const AGENTS_DIR_NAME: &str = ".agents";
pub const OLD_AEMEATH_DIR_NAME: &str = ".aemeath";
pub const SKILLS_DIR_NAME: &str = "skills";
pub const LOGS_DIR_NAME: &str = "logs";

pub fn global_agents_dir() -> PathBuf {
    if let Ok(value) = std::env::var(AGENTS_DIR_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return expand_home(Path::new(trimmed));
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(AGENTS_DIR_NAME)
}

pub fn global_config_path() -> PathBuf {
    global_agents_dir().join(NEW_CONFIG_FILE)
}

pub fn old_global_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(OLD_AEMEATH_DIR_NAME)
        .join(OLD_CONFIG_FILE)
}

pub fn project_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(AGENTS_DIR_NAME).join(NEW_CONFIG_FILE)
}

pub fn old_project_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(OLD_AEMEATH_DIR_NAME).join(OLD_CONFIG_FILE)
}

pub fn global_agents_md_path() -> PathBuf {
    global_agents_dir().join(AGENTS_MD)
}

pub fn old_global_claude_md_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join(CLAUDE_MD)
}

pub fn project_agents_md_path(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_MD)
}

pub fn old_project_claude_md_path(cwd: &Path) -> PathBuf {
    cwd.join(CLAUDE_MD)
}

pub fn global_skills_dir() -> PathBuf {
    global_agents_dir().join(SKILLS_DIR_NAME)
}

pub fn global_logs_dir() -> PathBuf {
    global_agents_dir().join(LOGS_DIR_NAME)
}

pub fn old_global_skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(OLD_AEMEATH_DIR_NAME)
        .join(SKILLS_DIR_NAME)
}

pub fn project_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub fn old_project_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(OLD_AEMEATH_DIR_NAME).join(SKILLS_DIR_NAME)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_project_paths_use_agents_directory() {
        let cwd = PathBuf::from("/tmp/demo");
        assert_eq!(
            project_config_path(&cwd),
            PathBuf::from("/tmp/demo/.agents/aemeath.json")
        );
        assert_eq!(
            project_agents_md_path(&cwd),
            PathBuf::from("/tmp/demo/AGENTS.md")
        );
        assert_eq!(
            project_skills_dir(&cwd),
            PathBuf::from("/tmp/demo/.agents/skills")
        );
    }

    #[test]
    fn test_global_logs_dir_uses_agents_logs_directory() {
        let temp_agents_dir = std::env::temp_dir().join(format!(
            "aemeath_agents_logs_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let previous = std::env::var_os(AGENTS_DIR_ENV);
        std::env::set_var(AGENTS_DIR_ENV, &temp_agents_dir);

        assert_eq!(global_logs_dir(), temp_agents_dir.join("logs"));

        if let Some(previous) = previous {
            std::env::set_var(AGENTS_DIR_ENV, previous);
        } else {
            std::env::remove_var(AGENTS_DIR_ENV);
        }
    }

    #[test]
    fn test_old_project_paths_use_aemeath_and_claude() {
        let cwd = PathBuf::from("/tmp/demo");
        assert_eq!(
            old_project_config_path(&cwd),
            PathBuf::from("/tmp/demo/.aemeath/config.json")
        );
        assert_eq!(
            old_project_claude_md_path(&cwd),
            PathBuf::from("/tmp/demo/CLAUDE.md")
        );
        assert_eq!(
            old_project_skills_dir(&cwd),
            PathBuf::from("/tmp/demo/.aemeath/skills")
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
