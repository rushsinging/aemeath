use chrono::Utc;

use super::{current_local_time_block, fixed_local_test_now, local_now};

#[test]
fn current_local_time_block_zh_labels_fixed_timestamp_and_stays_uncached() {
    let block = current_local_time_block("zh", fixed_local_test_now());

    assert_eq!(block.kind, "current_local_time");
    assert_eq!(block.content, "当前本地时间: 2026-06-15 14:30:05 +0800");
    assert!(!block.cacheable);
    assert!(!block.cache_break);
}

#[test]
fn current_local_time_block_english_labels_fixed_timestamp() {
    let block = current_local_time_block("en", fixed_local_test_now());

    assert_eq!(
        block.content,
        "Current local time: 2026-06-15 14:30:05 +0800"
    );
}

#[test]
fn current_local_time_block_defaults_unknown_language_to_english() {
    let block = current_local_time_block("fr", fixed_local_test_now());

    assert_eq!(
        block.content,
        "Current local time: 2026-06-15 14:30:05 +0800"
    );
}

#[test]
fn local_now_tracks_current_utc_clock_within_seconds() {
    let observed = local_now().naive_utc();
    let reference = Utc::now().naive_utc();
    let drift_seconds = (observed - reference).num_seconds().abs();

    assert!(
        drift_seconds <= 5,
        "local_now 与 UTC 参考时钟漂移 {drift_seconds}s，超出 5s 容差"
    );
}
