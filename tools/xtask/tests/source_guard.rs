use std::path::Path;

#[test]
fn test_only_api_guard_accepts_cfg_test_helper() {
    let source = "#[cfg(test)]\npub(crate) fn helper_for_test() {}";
    assert!(
        xtask::source_guard::find_test_only_api_violations(Path::new("x.rs"), source).is_empty()
    );
}

#[test]
fn test_only_api_guard_rejects_production_helper() {
    let source = "pub(crate) fn helper_for_test() {}";
    let violations = xtask::source_guard::find_test_only_api_violations(Path::new("x.rs"), source);
    assert_eq!(violations.len(), 1);
}

#[test]
fn test_only_api_guard_rejects_unguarded_testing_module() {
    let source = "mod testing;\n#[cfg(test)] mod fixture;";
    let violations = xtask::source_guard::find_test_only_api_violations(Path::new("x.rs"), source);
    assert_eq!(violations, vec!["x.rs: testing"]);
}

#[test]
fn dead_code_guard_counts_allow_attributes() {
    let source =
        "#[allow(dead_code)]\nfn legacy() {}\n#[cfg(test)]\n#[allow(dead_code)]\nfn fixture() {}";
    assert_eq!(
        xtask::source_guard::production_dead_code_allow_count(source),
        1
    );
}

#[test]
fn public_surface_is_sorted_and_excludes_cfg_test() {
    let source = "pub fn zebra() {}\n#[cfg(test)] pub fn alpha() {}\npub struct Beta;\nimpl Beta { pub fn build() {} }";
    let surface = xtask::source_guard::public_surface(Path::new("x.rs"), source);
    assert_eq!(
        surface,
        vec![
            "x.rs: pub fn Beta::build",
            "x.rs: pub fn zebra",
            "x.rs: pub struct Beta",
        ]
    );
}
