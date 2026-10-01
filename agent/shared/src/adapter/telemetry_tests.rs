use super::*;

#[test]
fn test_telemetry_adapter_new_wraps_inner() {
    let adapter = TelemetryAdapter::new("telemetry");

    assert_eq!(adapter.0, "telemetry");
}
