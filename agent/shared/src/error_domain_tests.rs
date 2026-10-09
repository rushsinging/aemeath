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

#[test]
fn with_code_sets_optional_fine_grained_discriminator() {
    // 粗分类（category）服务控制流重试判定；细分码（code）服务消费方
    // 需要变体粒度的判别（如 resume 失败分类转译），不参与重试语义。
    let bare = DomainError::invalid("context", "Session 不存在");
    assert_eq!(bare.code(), None);

    let coded = DomainError::invalid("context", "Session 不存在").with_code("session_not_found");
    assert_eq!(coded.code(), Some("session_not_found"));
    assert_eq!(coded.category(), ErrorCategory::Invalid);
}

#[test]
fn equality_includes_code() {
    let left = DomainError::invalid("context", "消息").with_code("code_a");
    let right = DomainError::invalid("context", "消息").with_code("code_b");
    let same = DomainError::invalid("context", "消息").with_code("code_a");
    assert_ne!(left, right);
    assert_eq!(left, same);
    // 无码与有码不等价
    assert_ne!(DomainError::invalid("context", "消息"), same);
}
