use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub fn memory_base_dir() -> PathBuf {
    PathBuf::from(share::config::paths::AGENTS_DIR_NAME)
        .join(share::config::paths::MEMORY_DIR_NAME)
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
        assert_eq!(memory_base_dir(), PathBuf::from(".agents/memory"));
    }
}
