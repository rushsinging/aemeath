//! 文件/目录建议生成

use std::path::PathBuf;

use super::types::{Suggestion, SuggestionType};

/// 根据部分路径生成文件/目录建议
pub fn generate_file_suggestions(partial: &str, cwd: &PathBuf) -> Vec<Suggestion> {
    // 移除 @ 前缀
    let path_str = if partial.starts_with('@') {
        partial.strip_prefix('@').unwrap_or(partial)
    } else {
        partial
    };

    if path_str.is_empty() {
        // 返回当前目录内容
        return list_directory_contents(cwd);
    }

    // 解析路径
    let path = if path_str.starts_with('/') {
        PathBuf::from(path_str)
    } else if path_str.starts_with('~') {
        // 展开主目录
        if let Some(home) = std::env::var("HOME").ok() {
            PathBuf::from(home).join(path_str.strip_prefix('~').unwrap_or(""))
        } else {
            cwd.join(path_str)
        }
    } else if path_str.starts_with('.') {
        cwd.join(path_str)
    } else {
        cwd.join(path_str)
    };

    // 获取要列出的目录和过滤前缀
    let (dir_to_list, filter_prefix) = if path.is_dir() {
        (path, "".to_string())
    } else {
        let parent = path.parent();
        let filename = path.file_name();
        match (parent, filename) {
            (Some(p), Some(f)) => (p.to_path_buf(), f.to_string_lossy().to_string()),
            (None, Some(f)) => (cwd.clone(), f.to_string_lossy().to_string()),
            _ => (cwd.clone(), "".to_string()),
        }
    };

    list_and_filter_directory(&dir_to_list, &filter_prefix, cwd)
}

/// 列出目录内容
pub fn list_directory_contents(dir: &PathBuf) -> Vec<Suggestion> {
    list_and_filter_directory(dir, "", dir)
}

/// 列出目录内容并按前缀过滤
pub fn list_and_filter_directory(
    dir: &PathBuf,
    prefix: &str,
    base_dir: &PathBuf,
) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();

    if !dir.exists() || !dir.is_dir() {
        return suggestions;
    }

    let entries = std::fs::read_dir(dir);
    if entries.is_err() {
        return suggestions;
    }

    let prefix_lower = prefix.to_lowercase();

    if let Ok(entries) = entries {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // 按前缀过滤
            if !prefix_lower.is_empty() && !name.to_lowercase().starts_with(&prefix_lower) {
                continue;
            }

            // 跳过隐藏文件（除非前缀以 . 开头）
            if name.starts_with('.') && !prefix_lower.starts_with('.') {
                continue;
            }

            let is_dir = path.is_dir();

            // 计算从 base_dir 的相对路径用于显示
            let display_path = if path.starts_with(base_dir) {
                path.strip_prefix(base_dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string()
            } else {
                path.to_string_lossy().to_string()
            };

            suggestions.push(Suggestion {
                _id: if is_dir {
                    format!("dir-{}", display_path)
                } else {
                    format!("file-{}", display_path)
                },
                display_text: if is_dir {
                    format!("{}{}", display_path, "/")
                } else {
                    display_path
                },
                _description: if is_dir {
                    Some("directory".to_string())
                } else {
                    None
                },
                suggestion_type: if is_dir {
                    SuggestionType::Directory
                } else {
                    SuggestionType::File
                },
            });
        }
    }

    // 排序：目录优先，然后文件，按字母顺序
    suggestions.sort_by(|a, b| match (&a.suggestion_type, &b.suggestion_type) {
        (SuggestionType::Directory, SuggestionType::File) => std::cmp::Ordering::Less,
        (SuggestionType::File, SuggestionType::Directory) => std::cmp::Ordering::Greater,
        _ => a.display_text.cmp(&b.display_text),
    });

    // 限制最多 15 条建议
    suggestions.truncate(15);
    suggestions
}
