//! 压缩后文件恢复附件构建

use std::collections::HashSet;
use crate::token_estimation::estimate_tokens;

/// 压缩后恢复的最大最近读取文件数。
pub const POST_COMPACT_MAX_FILES: usize = 5;

/// 每个恢复文件的最大 token 数。
pub const POST_COMPACT_MAX_TOKENS_PER_FILE: usize = 5_000;

/// 所有恢复文件的总 token 预算。
pub const POST_COMPACT_TOKEN_BUDGET: usize = 50_000;

/// 从最近读取的文件路径集合构建文件恢复附件。
/// 按修改时间排序读取最新的文件（不超过预算），返回要注入的摘要消息。
pub fn build_file_restoration(read_files: &HashSet<String>) -> Option<String> {
    if read_files.is_empty() {
        return None;
    }

    // 收集文件及其修改时间，按最近优先排序
    let mut files_with_mtime: Vec<(String, std::time::SystemTime)> = read_files
        .iter()
        .filter_map(|path| {
            let metadata = std::fs::metadata(path).ok()?;
            let mtime = metadata.modified().ok()?;
            Some((path.clone(), mtime))
        })
        .collect();

    files_with_mtime.sort_by(|a, b| b.1.cmp(&a.1));

    let mut restored_content = String::new();
    let mut total_tokens = 0usize;
    let mut file_count = 0usize;

    for (path, _mtime) in files_with_mtime.iter().take(POST_COMPACT_MAX_FILES) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let file_tokens = estimate_tokens(&content);
        let truncated = if file_tokens > POST_COMPACT_MAX_TOKENS_PER_FILE {
            let max_chars = POST_COMPACT_MAX_TOKENS_PER_FILE * 4; // ~4 字符/token
            let end = max_chars.min(content.len());
            let mut boundary = end;
            while boundary > 0 && !content.is_char_boundary(boundary) {
                boundary -= 1;
            }
            format!(
                "{}...\n[truncated, {} total chars]",
                &content[..boundary],
                content.len()
            )
        } else {
            content
        };

        let entry_tokens = estimate_tokens(&truncated) + 20; // 标签开销
        if total_tokens + entry_tokens > POST_COMPACT_TOKEN_BUDGET {
            break;
        }

        restored_content.push_str(&format!("\n<file path=\"{path}\">\n{truncated}\n</file>\n"));
        total_tokens += entry_tokens;
        file_count += 1;
    }

    if file_count == 0 {
        return None;
    }

    Some(format!(
        "<system-reminder>\n[Post-compaction file restoration: {} recently-read files]\n{restored_content}\n</system-reminder>",
        file_count
    ))
}
