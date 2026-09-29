//! `xtask guard` CLI 级测试：经编译产物二进制验证 exit 语义与违规输出格式。
//!
//! - clean 仓库 exit 0，stdout 报告 `0 violations`
//! - 故意违规 exit 2，stderr 输出 `rule_id + file:line`
//! - `--rule <id>` 定向执行单条规则

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GUARD_BIN: &str = env!("CARGO_BIN_EXE_xtask");

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent");
    fs::write(path, content).expect("write file");
}

fn write_registry(root: &Path, rules: serde_json::Value) {
    let registry = serde_json::json!({
        "version": 1,
        "budgets": {"repository_migration_debt": 0, "modules": {}},
        "entries": [],
        "rules": rules,
        "retired_symbols": []
    });
    write(
        &root.join(".agents/architecture-guard-registry.json"),
        &serde_json::to_string_pretty(&registry).expect("serialize"),
    );
}

fn run_guard(root: &Path, extra_args: &[&str]) -> std::process::Output {
    let mut args = vec!["guard"];
    args.extend_from_slice(extra_args);
    Command::new(GUARD_BIN)
        .args(&args)
        .env("AEMEATH_PROJECT_DIR", root)
        .output()
        .expect("spawn xtask guard")
}

fn clean_fixture() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path().join("repo");
    write_registry(&root, serde_json::json!([]));
    write(
        &root.join("agent/features/task/src/lib.rs"),
        "pub struct Task;\n",
    );
    (temp, root)
}

#[test]
fn guard_cli_clean_fixture_exits_zero() {
    let (_temp, root) = clean_fixture();

    let output = run_guard(&root, &[]);

    assert!(
        output.status.success(),
        "clean 仓库必须 exit 0，stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 violations"),
        "stdout 应报告 0 violations: {stdout}"
    );
}

#[test]
fn guard_cli_violating_fixture_exits_two_with_rule_id_and_location() {
    let (_temp, root) = clean_fixture();
    write_registry(
        &root,
        serde_json::json!([{
            "id": "use.feature.no-internal-segments",
            "assertion": "forbidden_segments",
            "scope": { "kind": "path_prefix", "value": "agent" },
            "forbidden_segments": ["domain"],
            "reason": "内部层禁穿透",
            "profile": "fast"
        }]),
    );
    write(
        &root.join("agent/features/runtime/src/service.rs"),
        "use task::domain::Entity;\n",
    );

    let output = run_guard(&root, &[]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "故意违规必须 exit 2，stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("use.feature.no-internal-segments"),
        "stderr 应含 rule_id: {stderr}"
    );
    assert!(
        stderr.contains("agent/features/runtime/src/service.rs:1"),
        "stderr 应含 file:line: {stderr}"
    );
}

#[test]
fn guard_cli_rule_filter_runs_only_selected_rule() {
    let (_temp, root) = clean_fixture();
    write_registry(
        &root,
        serde_json::json!([
            {
                "id": "use.feature.no-internal-segments",
                "assertion": "forbidden_segments",
                "scope": { "kind": "path_prefix", "value": "agent" },
                "forbidden_segments": ["domain"],
                "reason": "内部层禁穿透",
                "profile": "fast"
            },
            {
                "id": "layout.task.top-level",
                "assertion": "layout",
                "scope": { "kind": "path_prefix", "value": "agent/features/task/src" },
                "allowed_entries": ["lib.rs"],
                "reason": "顶层布局",
                "profile": "full"
            }
        ]),
    );
    // 该文件同时违反 layout 规则，但 --rule 定向只跑 forbidden_segments。
    write(
        &root.join("agent/features/runtime/src/service.rs"),
        "use task::domain::Entity;\n",
    );
    write(
        &root.join("agent/features/task/src/rogue.rs"),
        "pub struct Rogue;\n",
    );

    let output = run_guard(&root, &["--rule", "use.feature.no-internal-segments"]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("use.feature.no-internal-segments"));
    assert!(
        !stderr.contains("layout.task.top-level"),
        "--rule 定向不应执行其他规则: {stderr}"
    );
}
