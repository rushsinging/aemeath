use super::map_reader_error;
use crate::domain::session::SessionGenerationWireError;
use crate::SessionManagementError;

/// 会话不存在 → `NotFound`；存在但解码失败 → `Corrupt`；版本过新 → 原样上抛。
#[test]
fn reader_errors_are_mapped_to_distinct_session_management_errors() {
    assert!(matches!(
        map_reader_error("sess-1", SessionGenerationWireError::NotFound),
        SessionManagementError::NotFound(id) if id == "sess-1"
    ));

    assert!(matches!(
        map_reader_error(
            "sess-1",
            SessionGenerationWireError::InvalidManifest("筛选失败".to_string())
        ),
        SessionManagementError::Corrupt(message) if message.contains("筛选失败")
    ));

    assert!(matches!(
        map_reader_error(
            "sess-1",
            SessionGenerationWireError::UnsupportedFutureVersion {
                version: 99,
                original_bytes: Vec::new(),
            }
        ),
        SessionManagementError::UnsupportedFutureVersion(99)
    ));
}
