//! `agent/features/config/src/connect/outcome.rs` 的契约测试。

use crate::connect::outcome::ConnectOutcome;

#[test]
fn completed_label_distinguishes_from_cancelled() {
    assert_eq!(
        ConnectOutcome::Completed {
            applied_revision: 1
        }
        .kind_label(),
        "completed",
    );
    assert_eq!(ConnectOutcome::Cancelled.kind_label(), "cancelled");
}
