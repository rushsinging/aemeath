use super::error::{DomainError, ErrorCategory};

#[test]
fn constructors_set_category_and_domain() {
    let invalid = DomainError::invalid("project", "路径不在工作区根内");
    assert_eq!(invalid.category(), ErrorCategory::Invalid);
    assert_eq!(invalid.domain(), "project");
    assert_eq!(invalid.to_string(), "路径不在工作区根内");

    let storage = DomainError::storage("config", "读取失败");
    assert_eq!(storage.category(), ErrorCategory::Storage);
}

#[test]
fn category_matches_share_convention() {
    assert_eq!(ErrorCategory::Storage.as_str(), "storage");
    assert_eq!(ErrorCategory::Invalid.as_str(), "invalid");
    assert_eq!(ErrorCategory::Unavailable.as_str(), "unavailable");
}
