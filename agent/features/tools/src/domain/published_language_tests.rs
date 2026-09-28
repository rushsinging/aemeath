use super::published_language::*;

#[test]
fn tool_outcome_exposes_timeout_and_unconfirmed_terminals() {
    let timed_out =
        ToolOutcome::timed_out("达到 effective deadline", CleanupConfirmation::Confirmed);
    let unconfirmed = ToolOutcome::cancellation_unconfirmed(
        "底层工作未确认停止",
        vec!["外部进程可能仍在运行".to_string()],
        vec!["call-child-1".to_string()],
    );

    assert!(matches!(
        timed_out,
        ToolOutcome::TimedOut(ToolTerminalDetails {
            cleanup: CleanupConfirmation::Confirmed,
            ..
        })
    ));
    assert!(matches!(
        unconfirmed,
        ToolOutcome::CancellationUnconfirmed(ToolTerminalDetails {
            cleanup: CleanupConfirmation::Unconfirmed,
            ..
        })
    ));
}

#[test]
fn timeout_terminal_preserves_safe_diagnostics() {
    let outcome = ToolOutcome::CancellationUnconfirmed(ToolTerminalDetails {
        safe_reason: "远端未确认取消".to_string(),
        possible_side_effects: vec!["请求可能已提交".to_string()],
        unfinished_call_ids: vec!["remote-42".to_string()],
        cleanup: CleanupConfirmation::Unconfirmed,
    });

    let ToolOutcome::CancellationUnconfirmed(details) = outcome else {
        panic!("应为 CancellationUnconfirmed");
    };
    assert_eq!(details.safe_reason, "远端未确认取消");
    assert_eq!(details.possible_side_effects, ["请求可能已提交"]);
    assert_eq!(details.unfinished_call_ids, ["remote-42"]);
}

fn assert_same_type<T>(_: T, _: T) {}

#[test]
fn tool_vocab_root_reexports_share_definitions() {
    assert_same_type(
        share::tools_vocab::ToolName::new("Grep"),
        ToolName::new("Grep"),
    );
    assert_same_type(
        share::tools_vocab::ToolCapability::Read,
        ToolCapability::Read,
    );
    assert_same_type(
        share::tools_vocab::ToolCapabilities::empty(),
        ToolCapabilities::empty(),
    );
    assert_same_type(
        share::tools_vocab::AuthorizationContext::STANDARD,
        crate::domain::AuthorizationContext::STANDARD,
    );
}

#[test]
fn tool_vocab_capabilities_bit_identity_matches_share() {
    assert_eq!(
        ToolCapabilities::from(ToolCapability::All).bits(),
        share::tools_vocab::ToolCapabilities::from(share::tools_vocab::ToolCapability::All).bits()
    );
}

// ── ConcurrencyDeclaration ─────────────────────────────────────

#[test]
fn test_concurrency_safe_construction() {
    let safe = ConcurrencyDeclaration::safe();
    assert_eq!(safe.safety, ConcurrencySafety::Safe);
    assert!(safe.safety == ConcurrencySafety::Safe);
}

#[test]
fn test_concurrency_serialized_construction() {
    let serialized = ConcurrencyDeclaration::serialized();
    assert_eq!(serialized.safety, ConcurrencySafety::Serialized);
}

#[test]
fn test_concurrency_default_is_serialized() {
    assert_eq!(
        ConcurrencyDeclaration::default().safety,
        ConcurrencySafety::Serialized
    );
}

// ── CancellationDeclaration ────────────────────────────────────

#[test]
fn test_cancellation_variants() {
    assert_ne!(
        CancellationDeclaration::Cooperative,
        CancellationDeclaration::NonCooperative
    );
}

// ── ToolDescriptor ─────────────────────────────────────────────

#[test]
fn test_descriptor_is_concurrency_safe() {
    let desc = ToolDescriptor {
        name: ToolName::new("Glob"),
        description: "File glob tool".into(),
        input_schema: serde_json::json!({"type": "object"}),
        required_capabilities: ToolCapabilities::Read,
        concurrency: ConcurrencyDeclaration::safe(),
        cancellation: CancellationDeclaration::Cooperative,
        timeout_secs: 120,
        read_only: true,
        input_safety: InputSafetyDeclaration::Always,
        data_schema: serde_json::Value::Null,
    };
    assert!(desc.is_concurrency_safe());
    assert!(desc.is_cooperative_cancel());
}

#[test]
fn test_descriptor_serialized_and_non_cooperative() {
    let desc = ToolDescriptor {
        name: ToolName::new("Bash"),
        description: "Shell tool".into(),
        input_schema: serde_json::json!({"type": "object"}),
        required_capabilities: ToolCapabilities::Execute | ToolCapabilities::Write,
        concurrency: ConcurrencyDeclaration::serialized(),
        cancellation: CancellationDeclaration::NonCooperative,
        timeout_secs: 120,
        read_only: false,
        input_safety: InputSafetyDeclaration::Never,
        data_schema: serde_json::Value::Null,
    };
    assert!(!desc.is_concurrency_safe());
    assert!(!desc.is_cooperative_cancel());
}

// ── ToolInvocation ─────────────────────────────────────────────

// ── ToolErrorKind ──────────────────────────────────────────────

#[test]
fn test_tool_error_kind_equality() {
    assert_eq!(
        ToolErrorKind::ToolUnavailable,
        ToolErrorKind::ToolUnavailable
    );
    assert_ne!(ToolErrorKind::InvalidInput, ToolErrorKind::Internal);
}

// ── ToolOutcome ────────────────────────────────────────────────

#[test]
fn test_outcome_success_text() {
    let o = ToolOutcome::success_text("done");
    assert!(o.is_success());
    assert!(!o.is_failure());
    assert!(!o.is_cancelled());
    match o {
        ToolOutcome::Success(s) => {
            assert_eq!(s.content.len(), 1);
            assert_eq!(s.content[0].text, "done");
        }
        _ => panic!("应为 Success"),
    }
}

#[test]
fn test_outcome_failure_unavailable() {
    let o = ToolOutcome::failure(ToolErrorKind::ToolUnavailable, "not found");
    assert!(o.is_failure());
    match o {
        ToolOutcome::Failure(f) => {
            assert_eq!(f.kind, ToolErrorKind::ToolUnavailable);
            assert!(!f.retryable);
        }
        _ => panic!("应为 Failure"),
    }
}

#[test]
fn test_outcome_failure_internal_is_retryable() {
    let o = ToolOutcome::failure(ToolErrorKind::Internal, "oops");
    assert!(o.is_failure());
    match o {
        ToolOutcome::Failure(f) => assert!(f.retryable),
        _ => panic!("应为 Failure"),
    }
}

#[test]
fn test_outcome_cancelled() {
    let o = ToolOutcome::cancelled("user cancelled");
    assert!(o.is_cancelled());
    match o {
        ToolOutcome::Cancelled(c) => assert_eq!(c.reason, "user cancelled"),
        _ => panic!("应为 Cancelled"),
    }
}

#[test]
fn test_tool_failure_unavailable_helper() {
    let f = ToolFailure::unavailable("Agent");
    assert_eq!(f.kind, ToolErrorKind::ToolUnavailable);
    assert!(f.safe_message.contains("Agent"));
    assert!(!f.retryable);
}

#[test]
fn test_tool_failure_invalid_input_helper() {
    let f = ToolFailure::invalid_input("missing field: path");
    assert_eq!(f.kind, ToolErrorKind::InvalidInput);
    assert!(!f.retryable);
}

#[test]
fn test_tool_failure_retryable_classification() {
    assert!(!ToolFailure::new(ToolErrorKind::ToolUnavailable, "").retryable);
    assert!(!ToolFailure::new(ToolErrorKind::InvalidInput, "").retryable);
    assert!(!ToolFailure::new(ToolErrorKind::Unauthorized, "").retryable);
    assert!(ToolFailure::new(ToolErrorKind::ResourceUnavailable, "").retryable);
    assert!(ToolFailure::new(ToolErrorKind::Internal, "").retryable);
}

// ── RegistryScopeName / ToolProfileName ────────────────────────

#[test]
fn test_registry_scope_name_display() {
    let s = RegistryScopeName::new("main");
    assert_eq!(s.as_str(), "main");
    assert_eq!(format!("{s}"), "main");
}

#[test]
fn test_tool_profile_name_display() {
    let p = ToolProfileName::new("full");
    assert_eq!(p.as_str(), "full");
    assert_eq!(format!("{p}"), "full");
}

// ── ToolCatalogSnapshot ────────────────────────────────────────

#[test]
fn test_catalog_snapshot_find() {
    let desc1 = ToolDescriptor {
        name: ToolName::new("Read"),
        description: "Read tool".into(),
        input_schema: serde_json::json!({"type": "object"}),
        required_capabilities: ToolCapabilities::Read,
        concurrency: ConcurrencyDeclaration::safe(),
        cancellation: CancellationDeclaration::Cooperative,
        timeout_secs: 120,
        read_only: true,
        input_safety: InputSafetyDeclaration::Always,
        data_schema: serde_json::Value::Null,
    };
    let desc2 = ToolDescriptor {
        name: ToolName::new("Bash"),
        description: "Bash tool".into(),
        input_schema: serde_json::json!({"type": "object"}),
        required_capabilities: ToolCapabilities::Execute,
        concurrency: ConcurrencyDeclaration::serialized(),
        cancellation: CancellationDeclaration::NonCooperative,
        timeout_secs: 120,
        read_only: false,
        input_safety: InputSafetyDeclaration::Never,
        data_schema: serde_json::Value::Null,
    };
    let snapshot = ToolCatalogSnapshot::new("main", "full", vec![desc1, desc2]);

    assert_eq!(snapshot.len(), 2);
    assert!(!snapshot.is_empty());
    assert!(snapshot.find(&ToolName::new("read")).is_some());
    assert!(snapshot.find(&ToolName::new("READ")).is_some());
    assert!(snapshot.find(&ToolName::new("grep")).is_none());
}

#[test]
fn test_catalog_snapshot_empty() {
    let snapshot = ToolCatalogSnapshot::new("sub", "restricted", vec![]);
    assert!(snapshot.is_empty());
    assert_eq!(snapshot.len(), 0);
}

// ── ToolCatalogError ───────────────────────────────────────────

#[test]
fn test_catalog_error_display() {
    let e = ToolCatalogError::UnknownScope {
        scope: "xyz".into(),
    };
    assert!(e.to_string().contains("xyz"));
}
