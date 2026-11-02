use super::*;

#[test]
fn parallel_efficiency_reports_serial_waiting_below_concurrency() {
    // 5 块 × 60s = 300s 墙钟：理想并发 5 → 效率 1.0；实际串行 → 效率 5。
    let serial = CompactMetricsData {
        chunk_count: 5,
        concurrency: 5,
        chunk_ms_sum: 300_000,
        map_ms: 300_000,
        ..Default::default()
    };
    assert!((serial.parallel_efficiency() - 1.0).abs() < f64::EPSILON);

    let parallel = CompactMetricsData {
        chunk_count: 5,
        concurrency: 5,
        chunk_ms_sum: 300_000,
        map_ms: 60_000,
        ..Default::default()
    };
    assert!((parallel.parallel_efficiency() - 5.0).abs() < f64::EPSILON);

    let no_map = CompactMetricsData::default();
    assert_eq!(no_map.parallel_efficiency(), 0.0);
}

#[test]
fn compression_ratio_guards_zero_input() {
    let metrics = CompactMetricsData {
        early_tokens: 10_000,
        ..Default::default()
    };
    assert!((metrics.compression_ratio(2_500) - 0.25).abs() < f64::EPSILON);
    assert_eq!(CompactMetricsData::default().compression_ratio(1000), 0.0);
}

#[test]
fn default_metrics_mean_not_collected() {
    let metrics = CompactMetricsData::default();
    assert!(metrics.model.is_none());
    assert_eq!(metrics.chunk_count, 0);
    assert_eq!(metrics.waves, 0);
    assert_eq!(metrics.total_ms, 0);
}
