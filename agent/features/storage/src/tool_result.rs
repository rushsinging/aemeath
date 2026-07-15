//! Tool result persistence — saves large tool outputs to disk instead of keeping them in context.
//!
//! When a tool result exceeds `MAX_TOOL_RESULT_CHARS`, the full output is written to
//! `~/.agents/tool-results/{session_id}/{tool_use_id}.txt` and replaced in-context with
//! a compact `<persisted-output>` reference containing a head + tail preview.

use crate::LOG_TARGET;

use share::config::paths::session_tool_results_dir;
use share::string_idx::{slice_head, slice_tail};
use std::path::PathBuf;

/// 单条工具结果在落盘前的最大字符数。超过此值时写盘，消息里只放预览 + 文件指针。
///
/// 注意：本层只管"单条"tool result 的落盘阈值，不管"一个消息里多条 tool result 的总字符数"。
/// 例如一个 assistant 消息里塞了 5 条各 40k 的 tool result（合计 200k），每条单独看都未触发
/// 落盘，但总量可能撑爆上下文。这种"单消息多 tool result 总预算"的截断能力暂未实现；
/// 此前 runtime/compact/truncate.rs 中曾有相关草稿代码，但从未接入 compact 流程，已删除。
/// 真要支持时建议在本层（或 compact 流程内）重新设计，而非恢复旧代码。
pub const MAX_TOOL_RESULT_CHARS: usize = 50_000;

/// 落盘后保留的头部预览字符数。
const PREVIEW_HEAD: usize = 2_000;

/// 落盘后保留的尾部字符数。
const PREVIEW_TAIL: usize = 500;

/// Result of persisting a tool output to disk.
pub struct PersistedResult {
    /// Path where the full output was saved.
    pub filepath: PathBuf,
    /// Original size in bytes.
    pub original_size: usize,
    /// Head preview text.
    pub head: String,
    /// Tail preview text.
    pub tail: String,
}

/// Persist a large tool result to disk and return the replacement message.
///
/// If the output is small enough, returns `None` (no persistence needed).
/// On I/O error, returns `None` and logs a warning — the result stays inline.
pub fn persist_tool_result(
    session_id: &str,
    tool_use_id: &str,
    output: &str,
) -> Option<PersistedResult> {
    if output.len() <= MAX_TOOL_RESULT_CHARS {
        return None;
    }

    // Validate tool_use_id to prevent path traversal
    if tool_use_id.contains('/') || tool_use_id.contains('\\') || tool_use_id.contains("..") {
        log::warn!(target: LOG_TARGET,
            "rejecting tool_use_id with path separators: {}",
            tool_use_id
        );
        return None;
    }

    let dir = session_tool_results_dir(session_id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!(target: LOG_TARGET, "failed to create tool-results dir: {e}");
        return None;
    }

    let filepath = dir.join(format!("{tool_use_id}.txt"));

    // Write-once semantics: skip if already persisted (idempotent on resume)
    if !filepath.exists() {
        if let Err(e) = std::fs::write(&filepath, output) {
            log::error!(target: LOG_TARGET, "failed to persist tool result: {e}");
            return None;
        }
    }

    Some(PersistedResult {
        filepath,
        original_size: output.len(),
        head: slice_head(output, PREVIEW_HEAD).to_string(),
        tail: slice_tail(output, PREVIEW_TAIL).to_string(),
    })
}

/// Format a persisted result as an inline replacement message.
/// Uses `<persisted-output>` tags so the LLM knows the full output is on disk.
pub fn format_persisted_reference(result: &PersistedResult) -> String {
    let size_display = if result.original_size >= 1_000_000 {
        format!("{:.1} MB", result.original_size as f64 / 1_000_000.0)
    } else if result.original_size >= 1_000 {
        format!("{:.1} KB", result.original_size as f64 / 1_000.0)
    } else {
        format!("{} bytes", result.original_size)
    };

    format!(
        "<persisted-output>\nOutput too large ({size_display}). Full output saved to: {path}\n\n--- head ({head_len} chars) ---\n{head}\n\n[... {omitted} chars omitted ...]\n\n--- tail ({tail_len} chars) ---\n{tail}\n</persisted-output>",
        path = result.filepath.display(),
        head_len = result.head.len(),
        head = result.head,
        omitted = result.original_size - result.head.len() - result.tail.len(),
        tail_len = result.tail.len(),
        tail = result.tail,
    )
}

/// Process a list of tool result tuples: persist oversized results to disk,
/// replacing their output with a reference. Returns the number of results persisted.
pub fn persist_oversized_results(
    session_id: &str,
    results: &mut [(
        String,
        String,
        serde_json::Value,
        bool,
        Vec<share::tool::ImageData>,
    )],
) -> usize {
    let mut count = 0;
    for (tool_use_id, output, content, _is_error, _images) in results.iter_mut() {
        if let Some(persisted) = persist_tool_result(session_id, tool_use_id, output) {
            let reference = format_persisted_reference(&persisted);
            *output = reference.clone();
            *content = serde_json::json!({
                "text": reference,
                "persisted": {
                    "path": persisted.filepath,
                    "original_size": persisted.original_size,
                }
            });
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_result_not_persisted() {
        assert!(persist_tool_result("test", "id1", "small output").is_none());
    }

    #[test]
    fn test_oversized_result_persisted() {
        let session_id = format!("test-persist-{}", std::process::id());
        let oversized = "x".repeat(MAX_TOOL_RESULT_CHARS + 1);
        let result = persist_tool_result(&session_id, "id-oversized", &oversized);
        assert!(result.is_some());
        let persisted = result.unwrap();
        assert!(persisted.original_size > MAX_TOOL_RESULT_CHARS);
        assert!(!persisted.head.is_empty());
        assert!(!persisted.tail.is_empty());
        assert!(persisted.head.len() <= PREVIEW_HEAD);
        assert!(persisted.tail.len() <= PREVIEW_TAIL);
        // 清理
        let _ = std::fs::remove_dir_all(session_tool_results_dir(&session_id));
    }

    #[test]
    fn test_format_persisted_reference() {
        let result = PersistedResult {
            filepath: PathBuf::from("/tmp/test.txt"),
            original_size: 100_000,
            head: "first line\nsecond line".to_string(),
            tail: "last line".to_string(),
        };
        let formatted = format_persisted_reference(&result);
        assert!(formatted.contains("<persisted-output>"));
        assert!(formatted.contains("100.0 KB"));
        assert!(formatted.contains("/tmp/test.txt"));
        assert!(formatted.contains("first line"));
        assert!(formatted.contains("last line"));
        assert!(formatted.contains("chars omitted"));
    }
}
