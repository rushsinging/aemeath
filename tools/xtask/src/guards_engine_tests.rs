use std::fs;
use std::path::Path;

fn write_source(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent");
    fs::write(path, contents).expect("write source");
}

#[test]
fn engine_collects_plain_and_group_use_paths() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let source_path = temp.path().join("service.rs");
    write_source(
        &source_path,
        "use crate::domain::Entity;\nuse agent_share::{ports, adapters::wire};\npub fn f() {}\n",
    );

    let index = crate::guards_engine::index_file(&source_path).expect("index file");

    let paths: Vec<String> = index
        .use_paths
        .iter()
        .map(|path| path.text.clone())
        .collect();
    assert!(paths.contains(&"crate::domain::Entity".to_owned()));
    assert!(paths.contains(&"agent_share::ports".to_owned()));
    assert!(paths.contains(&"agent_share::adapters::wire".to_owned()));
}

#[test]
fn engine_marks_use_inside_test_module_as_non_production() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let source_path = temp.path().join("service.rs");
    write_source(
        &source_path,
        "use crate::domain::Entity;\n#[cfg(test)]\nmod tests {\n    use crate::application::Svc;\n}\n",
    );

    let index = crate::guards_engine::index_file(&source_path).expect("index file");

    let production: Vec<String> = index
        .production_use_paths()
        .into_iter()
        .map(|path| path.text.clone())
        .collect();
    assert!(production
        .iter()
        .any(|path| path == "crate::domain::Entity"));
    assert!(!production
        .iter()
        .any(|path| path == "crate::application::Svc"));
}

#[test]
fn engine_reports_source_line_for_each_use() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let source_path = temp.path().join("service.rs");
    write_source(&source_path, "\nuse crate::domain::Entity;\n");

    let index = crate::guards_engine::index_file(&source_path).expect("index file");

    let use_path = index
        .production_use_paths()
        .into_iter()
        .find(|path| path.text == "crate::domain::Entity")
        .expect("use must be collected");
    assert_eq!(use_path.line, 2);
}

#[test]
fn engine_collects_public_reexports_of_crate_root() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path().join("lib.rs");
    write_source(
        &root,
        "pub use inner::{Good, AlsoGood};\nmod inner;\npub struct Leaked;\n",
    );

    let index = crate::guards_engine::index_file(&root).expect("index file");

    assert_eq!(index.public_reexports, vec!["Good", "AlsoGood"]);
}

#[test]
fn engine_indexes_directory_recursively() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(&temp.path().join("a/one.rs"), "use crate::x::Y;\n");
    write_source(&temp.path().join("b/two.rs"), "use crate::x::Z;\n");

    let index = crate::guards_engine::index_directory(temp.path()).expect("index directory");

    assert_eq!(index.len(), 2);
}
