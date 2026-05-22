use crate::config::paths;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub fn memory_base_dir() -> PathBuf {
    paths::global_memory_dir()
}

pub fn project_hash(cwd: &str) -> String {
    let canonical = std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| PathBuf::from(cwd))
        .to_string_lossy()
        .to_string();
    stable_hash(&canonical)
}

pub fn project_hash_from_path(path: &Path) -> String {
    project_hash(&path.to_string_lossy())
}

fn stable_hash(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_hash_stable() {
        let first = project_hash("/tmp/aemeath-test");
        let second = project_hash("/tmp/aemeath-test");

        assert_eq!(first, second);
        assert_eq!(first.len(), 16);
    }

    #[test]
    fn test_project_hash_distinct() {
        let first = project_hash("/tmp/aemeath-test-a");
        let second = project_hash("/tmp/aemeath-test-b");

        assert_ne!(first, second);
    }

    #[test]
    fn test_memory_base_dir_uses_agents_directory() {
        let _guard = paths::TEST_ENV_LOCK.lock().unwrap();
        let temp_agents_dir = std::env::temp_dir().join(format!(
            "aemeath_memory_dir_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let previous = std::env::var_os(paths::AGENTS_DIR_ENV);
        std::env::set_var(paths::AGENTS_DIR_ENV, &temp_agents_dir);

        assert_eq!(memory_base_dir(), temp_agents_dir.join("memory"));

        if let Some(previous) = previous {
            std::env::set_var(paths::AGENTS_DIR_ENV, previous);
        } else {
            std::env::remove_var(paths::AGENTS_DIR_ENV);
        }
    }
}
