use std::path::PathBuf;

#[test]
fn coverage_summary_prints_workspace_and_longest_package_owner() {
    let root = PathBuf::from("/workspace");
    let report = r#"{
      "data": [{"files": [
        {"filename":"/workspace/agent/features/runtime/src/lib.rs","summary":{
          "regions":{"count":10,"covered":5},
          "functions":{"count":4,"covered":3},
          "lines":{"count":10,"covered":8}}},
        {"filename":"/workspace/apps/cli/src/main.rs","summary":{
          "regions":{"count":2,"covered":1},
          "functions":{"count":2,"covered":1},
          "lines":{"count":4,"covered":2}}}
      ]}]
    }"#;
    let metadata = r#"{"packages":[
      {"name":"runtime","manifest_path":"/workspace/agent/features/runtime/Cargo.toml"},
      {"name":"cli","manifest_path":"/workspace/apps/cli/Cargo.toml"},
      {"name":"audit","manifest_path":"/workspace/agent/features/audit/Cargo.toml"}
    ]}"#;

    let output = xtask::coverage::render_summary(report, metadata, &root).unwrap();

    assert!(output.contains("workspace"));
    assert!(output.contains("6/12 (50.00%)"));
    assert!(output.contains("runtime"));
    assert!(output.contains("8/10 (80.00%)"));
    assert!(output.contains("audit"));
    assert!(output.contains("n/a"));
}

#[test]
fn coverage_summary_rejects_missing_metric() {
    let report = r#"{"data":[{"files":[{"filename":"/workspace/src/lib.rs","summary":{}}]}]}"#;
    let metadata = r#"{"packages":[]}"#;

    let error = xtask::coverage::render_summary(report, metadata, &PathBuf::from("/workspace"))
        .unwrap_err();

    assert!(error.to_string().contains("regions"));
}
