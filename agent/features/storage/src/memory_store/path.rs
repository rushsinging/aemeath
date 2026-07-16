use std::path::{Path, PathBuf};

/// 全局 memory 目录：`~/.agents/memory`
pub fn memory_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(share::config::paths::AGENTS_DIR_NAME)
        .join(share::config::paths::MEMORY_DIR_NAME)
}

/// 根据项目路径生成可读文件名。
///
/// 规则：去掉开头的 `/`，将路径分隔符替换为 `-`。
/// 示例：`/Users/guoyuqi/work/aemeath` → `Users-guoyuqi-work-aemeath`
pub fn project_file_name(cwd: &str) -> String {
    let canonical = std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| PathBuf::from(cwd))
        .to_string_lossy()
        .to_string();
    canonical.trim_start_matches('/').replace('/', "-")
}

pub fn project_file_name_from_path(path: &Path) -> String {
    project_file_name(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_file_name_stable() {
        let first = project_file_name("/tmp/aemeath-test");
        let second = project_file_name("/tmp/aemeath-test");

        assert_eq!(first, second);
        assert!(!first.is_empty());
    }

    #[test]
    fn test_project_file_name_distinct() {
        let first = project_file_name("/tmp/aemeath-test-a");
        let second = project_file_name("/tmp/aemeath-test-b");

        assert_ne!(first, second);
    }

    #[test]
    fn test_project_file_name_readable() {
        let name = project_file_name("/Users/guoyuqi/work/aemeath");
        assert_eq!(name, "Users-guoyuqi-work-aemeath");
    }

    #[test]
    fn test_memory_base_dir_uses_home() {
        let dir = memory_base_dir();
        assert!(dir.ends_with(".agents/memory"));
        // 确保不是相对路径
        assert!(dir.is_absolute() || dir.starts_with("/"));
    }
}
