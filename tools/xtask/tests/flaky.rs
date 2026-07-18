#[test]
fn flaky_runner_preserves_first_failure_even_if_retry_passes() {
    let mut attempts = 0;
    let report = xtask::flaky::run_with_retry("demo", 1, || {
        attempts += 1;
        if attempts == 1 {
            1
        } else {
            0
        }
    });
    assert_eq!(report.first_exit, 1);
    assert_eq!(report.retry_exits, vec![0]);
    assert!(!report.passed);
    assert_eq!(report.classification, "flaky-suspect");
}
