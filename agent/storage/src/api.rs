pub use crate::logging::{
    append_json_line, append_json_line_with_turn, append_line, append_text_line,
    append_text_line_with_turn, format_text_line, format_text_line_with_turn, is_rotated_log_path,
    open_append, prepare_log_file, rotated_path, timestamp_rfc3339, JsonLogger, LogFile,
    LOG_MAX_BACKUPS, LOG_MAX_BYTES, LOG_RETENTION_DAYS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = StorageApiMarker;
        assert_eq!(marker, marker);
    }
}
