//! `agent/features/config/src/connect/draft.rs` 的契约测试。

use crate::connect::draft::{ConnectDraft, DraftValidationError, ModelDraft};

#[test]
fn model_draft_rejects_zero_window_or_max_tokens() {
    let zero_window = ModelDraft {
        model_id: "x".into(),
        context_window: 0,
        max_tokens: 1,
    };
    assert!(matches!(
        zero_window.validate(),
        Err(DraftValidationError::ZeroContextWindow)
    ));
    let zero_tokens = ModelDraft {
        model_id: "x".into(),
        context_window: 100,
        max_tokens: 0,
    };
    assert!(matches!(
        zero_tokens.validate(),
        Err(DraftValidationError::ZeroMaxTokens)
    ));
}

#[test]
fn model_draft_rejects_max_tokens_above_window() {
    let bad = ModelDraft {
        model_id: "x".into(),
        context_window: 100,
        max_tokens: 200,
    };
    assert!(matches!(
        bad.validate(),
        Err(DraftValidationError::MaxTokensExceedsContext { .. })
    ));
}

#[test]
fn normalize_provider_user_agent_clears_blank() {
    assert!(ConnectDraft::normalize_provider_user_agent(None).is_none());
    assert!(ConnectDraft::normalize_provider_user_agent(Some("   ")).is_none());
    assert_eq!(
        ConnectDraft::normalize_provider_user_agent(Some("  ua/1.0 ")).as_deref(),
        Some("ua/1.0"),
    );
}

#[test]
fn validate_provider_user_agent_rejects_control_characters() {
    let bad = ConnectDraft::validate_provider_user_agent(Some("ua\r\ninjected"));
    assert!(matches!(
        bad,
        Err(DraftValidationError::InvalidProviderUserAgent)
    ));
    assert_eq!(
        ConnectDraft::normalize_provider_user_agent(Some("ua\r\ninjected")).as_deref(),
        Some("ua\r\ninjected"),
    );
}

#[test]
fn normalize_base_url_rejects_blank_or_non_http_scheme() {
    assert!(matches!(
        ConnectDraft::normalize_base_url(""),
        Err(DraftValidationError::EmptyBaseUrl)
    ));
    assert!(matches!(
        ConnectDraft::normalize_base_url("   "),
        Err(DraftValidationError::EmptyBaseUrl)
    ));
    assert!(matches!(
        ConnectDraft::normalize_base_url("ftp://x"),
        Err(DraftValidationError::InvalidBaseUrlScheme)
    ));
    assert_eq!(
        ConnectDraft::normalize_base_url("  https://api.x.example/ ").unwrap(),
        "https://api.x.example/",
    );
}
