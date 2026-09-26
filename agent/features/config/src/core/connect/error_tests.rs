//! `agent/features/config/src/connect/error.rs` 的契约测试。

use crate::connect::error::ConnectError;
use crate::connect::states::{ConnectRevision, ConnectStage};

#[test]
fn invalid_transition_display_includes_command_and_stage() {
    let err = ConnectError::InvalidTransition {
        command: "ConfirmOverwrite",
        actual: ConnectStage::SelectProvider,
    };
    let message = err.display_message();
    assert!(message.contains("ConfirmOverwrite"));
    assert!(message.contains("SelectProvider"));
}

#[test]
fn stale_revision_display_warns_to_reload() {
    let err = ConnectError::StaleRevision {
        actual: ConnectRevision::from_value(3),
        provided: ConnectRevision::from_value(1),
    };
    let message = err.display_message();
    assert!(message.contains("重载") || message.contains("revision"));
}
