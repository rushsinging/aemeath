use super::*;

#[test]
fn test_error_display() {
    let error = AemeathError::Auth {
        message: "Invalid API key".to_string(),
    };
    let display = error.display();
    let msg = display.user_message();
    assert!(msg.contains("认证失败"));
}

#[test]
fn test_retryable() {
    let error = AemeathError::RateLimit { retry_after: 60 };
    assert!(error.is_retryable());

    let error = AemeathError::Auth {
        message: "test".to_string(),
    };
    assert!(!error.is_retryable());
}

#[test]
fn test_error_context_new_uses_injected_timestamp() {
    let context = ErrorContext::new("test-location", 123);

    assert_eq!(context.location, "test-location");
    assert_eq!(context.timestamp, 123);
    assert!(context.context.is_none());
}

#[test]
fn test_error_context_with_context() {
    let context = ErrorContext::new("test-location", 123).with_context("detail");

    assert_eq!(context.context.as_deref(), Some("detail"));
    assert_eq!(context.timestamp, 123);
}
