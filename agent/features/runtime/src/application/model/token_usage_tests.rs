use super::normalized_total_tokens;
use crate::ports::RawUsageSnapshotData;

#[test]
fn runtime_consumes_provider_normalized_total_without_readding_cache() {
    let usage = RawUsageSnapshotData {
        input_tokens: Some(100),
        output_tokens: Some(20),
        cache_read_tokens: Some(80),
        cache_write_tokens: Some(30),
        ..RawUsageSnapshotData::default()
    };

    assert_eq!(normalized_total_tokens(&usage), 120);
}
