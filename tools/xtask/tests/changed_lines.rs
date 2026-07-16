use std::collections::BTreeMap;

#[test]
fn parses_added_lines_from_unified_zero_diff() {
    let diff = "+++ b/src/lib.rs\n@@ -1,0 +2,2 @@\n+one\n+two\n";
    let lines = xtask::changed_lines::parse_diff(diff).unwrap();
    assert_eq!(lines["src/lib.rs"], vec![2, 3]);
}

#[test]
fn reports_covered_and_missing_changed_lines() {
    let coverage = r#"{"data":[{"files":[{"filename":"/repo/src/lib.rs","segments":[[2,0,1,true,true,false],[3,0,0,true,true,false]]}]}]}"#;
    let changed = BTreeMap::from([("src/lib.rs".to_owned(), vec![2, 3])]);
    let report =
        xtask::changed_lines::report(coverage, &changed, std::path::Path::new("/repo")).unwrap();
    assert_eq!(report.changed, 2);
    assert_eq!(report.covered, 1);
    assert_eq!(report.missing, vec!["src/lib.rs:3"]);
}
