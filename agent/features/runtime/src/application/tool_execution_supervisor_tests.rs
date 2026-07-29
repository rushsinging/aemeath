use super::tool_execution_supervisor::earliest_deadline;
use std::time::{Duration, SystemTime};

#[test]
fn supervisor_uses_earliest_deadline() {
    let now = SystemTime::now();
    assert_eq!(
        earliest_deadline(
            Some(now + Duration::from_secs(30)),
            Some(now + Duration::from_secs(20)),
            Some(now + Duration::from_secs(10)),
        ),
        Some(now + Duration::from_secs(10))
    );
}
