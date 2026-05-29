//! Tool result persistence — saves large tool outputs to disk instead of keeping them in context.
//!
//! When a tool result exceeds `MAX_TOOL_RESULT_CHARS`, the full output is written to
//! `~/.aemeath/tool-results/{session_id}/{tool_use_id}.txt` and replaced in-context with
//! a compact `<persisted-output>` reference containing a preview.

use std::path::PathBuf;

const MAX_TOOL_RESULT_CHARS: usize = 50_000;

/// Preview: how many bytes to keep from the beginning.
const PREVIEW_SIZE_BYTES: usize = 2_000;

/// Get the tool-results directory for a session.
fn tool_results_dir(session_id: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".aemeath").join("tool-results").join(session_id)
}

/// Result of persisting a tool output to disk.
pub struct PersistedResult {
    /// Path where the full output was saved.
    pub filepath: PathBuf,
    /// Original size in bytes.
    pub original_size: usize,
    /// Preview text (first N bytes, cut at newline boundary).
    pub preview: String,
}

/// Generate a newline-aware preview of content.
/// Prefers cutting at a newline if one exists in the last 50% of the window.
fn generate_preview(content: &str, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content.to_string(), false);
    }

    // Find a safe UTF-8 boundary
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &content[..end];

    // Prefer cutting at a newline in the last 50%
    if let Some(last_nl) = truncated.rfind('\n') {
        if last_nl > max_bytes / 2 {
            return (content[..last_nl].to_string(), true);
        }
    }

    (truncated.to_string(), true)
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
        log::warn!(
            "rejecting tool_use_id with path separators: {}",
            tool_use_id
        );
        return None;
    }

    let dir = tool_results_dir(session_id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("failed to create tool-results dir: {e}");
        return None;
    }

    let filepath = dir.join(format!("{tool_use_id}.txt"));

    // Write-once semantics: skip if already persisted (idempotent on resume)
    if !filepath.exists() {
        if let Err(e) = std::fs::write(&filepath, output) {
            log::warn!("failed to persist tool result: {e}");
            return None;
        }
    }

    let (preview, _has_more) = generate_preview(output, PREVIEW_SIZE_BYTES);

    Some(PersistedResult {
        filepath,
        original_size: output.len(),
        preview,
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
        "<persisted-output>\nOutput too large ({size_display}). Full output saved to: {path}\n\nPreview (first {preview_size} bytes):\n{preview}\n...\n</persisted-output>",
        path = result.filepath.display(),
        preview_size = result.preview.len(),
        preview = result.preview,
    )
}

/// Process a list of tool result tuples: persist oversized results to disk,
/// replacing their output with a reference. Returns the number of results persisted.
pub fn persist_oversized_results(
    session_id: &str,
    results: &mut [(String, String, bool, Vec<share::tool::ImageData>)],
) -> usize {
    let mut count = 0;
    for (tool_use_id, output, _is_error, _images) in results.iter_mut() {
        if let Some(persisted) = persist_tool_result(session_id, tool_use_id, output) {
            *output = format_persisted_reference(&persisted);
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_preview_short() {
        let (preview, has_more) = generate_preview("hello world", 100);
        assert_eq!(preview, "hello world");
        assert!(!has_more);
    }

    #[test]
    fn test_generate_preview_long() {
        let content = "line1\nline2\nline3\nline4\nline5\n".repeat(100);
        let (preview, has_more) = generate_preview(&content, 50);
        assert!(has_more);
        assert!(preview.len() <= 50);
        // Should be a proper substring of the original content
        assert!(content.starts_with(&preview));
    }

    #[test]
    fn test_small_result_not_persisted() {
        assert!(persist_tool_result("test", "id1", "small output").is_none());
    }

    #[test]
    fn test_format_persisted_reference() {
        let result = PersistedResult {
            filepath: PathBuf::from("/tmp/test.txt"),
            original_size: 100_000,
            preview: "first line\nsecond line".to_string(),
        };
        let formatted = format_persisted_reference(&result);
        assert!(formatted.contains("<persisted-output>"));
        assert!(formatted.contains("100.0 KB"));
        assert!(formatted.contains("/tmp/test.txt"));
        assert!(formatted.contains("first line"));
    }
}
