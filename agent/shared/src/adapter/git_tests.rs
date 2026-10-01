use super::*;

#[test]
fn test_git_adapter_new_wraps_inner() {
    let adapter = GitAdapter::new("git");

    assert_eq!(adapter.0, "git");
}
