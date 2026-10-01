use super::*;

#[test]
fn test_memory_error_file() {
    let err = MemoryError::file("/tmp/memory.json", std::io::Error::other("denied"));

    assert!(matches!(err, MemoryError::File { .. }));
    assert!(err.to_string().contains("/tmp/memory.json"));
    assert!(err.to_string().contains("denied"));
}

#[test]
fn test_memory_error_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let err = MemoryError::json(json_err);

    assert!(matches!(err, MemoryError::Json { .. }));
    assert!(err.to_string().contains("记忆 JSON 解析失败"));
}

#[test]
fn test_memory_error_not_found() {
    let err = MemoryError::not_found("mem-1");

    assert!(matches!(err, MemoryError::NotFound { .. }));
    assert_eq!(err.to_string(), "记忆不存在: mem-1");
}
