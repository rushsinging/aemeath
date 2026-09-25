use super::*;
use std::fs;

fn write_source(path: &std::path::Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    fs::write(path, contents).expect("write source");
}

fn mini_repo(temp: &std::path::Path) -> std::path::PathBuf {
    let root = temp.join("repo");
    // storage：导出 Live（跨 crate 消费）、Fold（内部折返）、Ghost（死）。
    write_source(
        &root.join("agent/features/storage/src/lib.rs"),
        "pub use domain::{Fold, Ghost, Live};\nmod domain;\n",
    );
    write_source(
        &root.join("agent/features/storage/src/domain.rs"),
        "pub struct Fold;\npub struct Ghost;\npub struct Live;\n",
    );
    write_source(
        &root.join("agent/features/storage/src/adapters/blob.rs"),
        "use crate::Fold;\nfn f(x: crate::Fold) -> crate::Fold { x }\n",
    );
    // 跨 crate 消费 Live。
    write_source(
        &root.join("agent/composition/src/app.rs"),
        "use storage::Live;\nfn g(l: Live) {}\n",
    );
    // 其余 crate 空壳。
    for crate_name in CRATES.iter().filter(|c| **c != "storage") {
        write_source(
            &root.join(format!("agent/features/{crate_name}/src/lib.rs")),
            "",
        );
    }
    root
}

#[test]
fn analyze_classifies_three_consumption_faces() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = mini_repo(temp.path());

    let report = analyze_crate(&root, "storage").expect("analyze");

    assert!(report.exports.contains_key("Live"));
    assert!(report.cross_consumed.contains("Live"), "跨 crate 消费");
    assert!(report.internal_root_consumed.contains("Fold"), "内部折返");
    let dead = dead_exports(&report);
    assert_eq!(
        dead,
        ["Ghost"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        "仅 Ghost 死"
    );
}

#[test]
fn rewrite_then_trim_is_atomic_and_idempotent() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = mini_repo(temp.path());

    let report = analyze_crate(&root, "storage").expect("analyze");

    // 步骤 1：折返改写。
    let rewritten = rewrite_internal_root_consumption(&root, "storage", &report).expect("rewrite");
    assert!(rewritten >= 2, "限定与组内折返均改写：{rewritten}");
    let blob_source =
        fs::read_to_string(root.join("agent/features/storage/src/adapters/blob.rs")).unwrap();
    assert!(
        blob_source.contains("crate::domain::Fold"),
        "改写为真实路径"
    );
    assert!(!blob_source.contains("crate::Fold"), "无折返残留");

    // 步骤 2：折返已改写后重算，Fold 变死 → 与 Ghost 一并下架。
    let report2 = analyze_crate(&root, "storage").expect("re-analyze");
    assert!(!report2.internal_root_consumed.contains("Fold"));
    let dead2 = dead_exports(&report2);
    assert!(dead2.contains("Ghost") && dead2.contains("Fold"));

    let removed = apply_trim(&root, "storage", &dead2).expect("trim");
    assert_eq!(removed, 2, "Fold 与 Ghost 各删一次：{removed}");
    let lib_source = fs::read_to_string(root.join("agent/features/storage/src/lib.rs")).unwrap();
    assert!(lib_source.contains("Live"), "Live 保留");
    assert!(
        !lib_source.contains("Ghost") && !lib_source.contains("Fold"),
        "死导出清除"
    );

    // 幂等：重复 apply 无变化。
    let removed_again = apply_trim(&root, "storage", &dead2).expect("re-trim");
    assert_eq!(removed_again, 0, "幂等");
}

#[test]
fn trim_handles_multiline_group_without_corruption() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path().join("repo");
    write_source(
        &root.join("agent/features/audit/src/lib.rs"),
        "pub use domain::{\n    Keep,\n    Kill,\n};\nmod domain;\n",
    );
    write_source(
        &root.join("agent/features/audit/src/domain.rs"),
        "pub struct Keep;\npub struct Kill;\n",
    );
    write_source(
        &root.join("agent/composition/src/app.rs"),
        "use audit::Keep;\n",
    );
    for crate_name in CRATES.iter().filter(|c| **c != "audit") {
        write_source(
            &root.join(format!("agent/features/{crate_name}/src/lib.rs")),
            "",
        );
    }

    let report = analyze_crate(&root, "audit").expect("analyze");
    let dead = dead_exports(&report);
    assert_eq!(
        dead,
        ["Kill"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
    );

    let removed = apply_trim(&root, "audit", &dead).expect("trim");
    assert_eq!(removed, 1);
    let lib_source = fs::read_to_string(root.join("agent/features/audit/src/lib.rs")).unwrap();
    assert!(lib_source.contains("Keep") && !lib_source.contains("Kill"));
    assert!(
        !lib_source.contains(",,") && !lib_source.contains("{\n}"),
        "无残留空组/双逗号"
    );
}
