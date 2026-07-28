use std::time::Duration;

use super::*;

#[test]
fn memory_baseline_tracks_first_peak_and_growth_samples() {
    let mut baseline = ProcessMemoryBaseline::new(Duration::ZERO);
    let now = Duration::from_secs(1);

    let first = baseline.observe(now, Some(100)).expect("first sample");
    assert_eq!(first.current_rss_bytes, 100);
    assert_eq!(first.peak_rss_bytes, 100);
    assert_eq!(first.first_rss_bytes, 100);
    assert_eq!(first.growth_from_first_bytes, 0);
    assert_eq!(first.growth_from_previous_bytes, 0);

    let grown = baseline
        .observe(now + Duration::from_secs(1), Some(175))
        .expect("grown sample");
    assert_eq!(grown.current_rss_bytes, 175);
    assert_eq!(grown.peak_rss_bytes, 175);
    assert_eq!(grown.growth_from_first_bytes, 75);
    assert_eq!(grown.growth_from_previous_bytes, 75);

    let shrunk = baseline
        .observe(now + Duration::from_secs(2), Some(125))
        .expect("shrunk sample");
    assert_eq!(shrunk.peak_rss_bytes, 175);
    assert_eq!(shrunk.growth_from_first_bytes, 25);
    assert_eq!(shrunk.growth_from_previous_bytes, -50);
}

#[test]
fn memory_baseline_throttles_samples_without_losing_previous_state() {
    let mut baseline = ProcessMemoryBaseline::new(Duration::from_secs(5));

    assert!(baseline.observe(Duration::ZERO, Some(100)).is_some());
    let cached = baseline
        .observe(Duration::from_secs(4), Some(200))
        .expect("throttled call returns cached snapshot");
    assert_eq!(cached.current_rss_bytes, 100);
    assert_eq!(cached.growth_from_previous_bytes, 0);
    let next = baseline
        .observe(Duration::from_secs(5), Some(150))
        .expect("sample after interval");
    assert_eq!(next.growth_from_previous_bytes, 50);
}

#[test]
fn memory_baseline_degrades_when_platform_sample_is_unavailable() {
    let mut baseline = ProcessMemoryBaseline::new(Duration::ZERO);

    assert_eq!(baseline.observe(Duration::ZERO, None), None);
    let first = baseline
        .observe(Duration::from_secs(1), Some(256))
        .expect("later supported sample");
    assert_eq!(first.first_rss_bytes, 256);
}
