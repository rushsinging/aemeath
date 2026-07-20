use std::fs;

#[test]
fn sdk_wire_schema_check_rejects_stale_committed_artifact() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let output = temp.path().join("wire-components.schema.json");
    fs::write(&output, "{}\n").expect("write stale schema");

    let error = crate::sdk_wire_schema::check(&output).expect_err("stale artifact must fail");

    assert!(error.to_string().contains("SDK Wire Schema 已过期"));
}

#[test]
fn sdk_wire_schema_write_creates_missing_parent_directories() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let output = temp.path().join("generated/wire-components.schema.json");

    crate::sdk_wire_schema::write(&output).expect("write must create parent directories");

    assert!(output.is_file());
}

#[test]
fn sdk_wire_schema_write_then_check_is_deterministic() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let output = temp.path().join("wire-components.schema.json");

    crate::sdk_wire_schema::write(&output).expect("write generated schema");
    crate::sdk_wire_schema::check(&output).expect("fresh artifact must pass");
}
