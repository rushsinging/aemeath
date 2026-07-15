use std::fs;
use std::path::Path;

#[test]
fn command_lists_reachability_subcommand() {
    let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))
        .expect("read main.rs");

    assert!(source.contains("production-reachability"));
}
