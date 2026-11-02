//! L5 系统 smoke：`aemeath eval compact` 三个离线子命令端到端可跑。
//!
//! 只覆盖「不调 LLM」的路径（telemetry / score），用真实二进制 + 临时 fixture：
//! - telemetry 必须能读旧格式日志并产出画像 JSON；
//! - score 必须能读 case 目录并在无 summary 时计入 skipped（分母规则）；
//! - 顶层 `eval compact --help` 必须可执行（命令树装配无回归）。

use std::path::PathBuf;
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("aemeath-eval-smoke-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn aemeath() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aemeath"))
}

#[test]
fn eval_compact_help_is_available() {
    let output = aemeath()
        .args(["eval", "compact", "--help"])
        .output()
        .expect("run help");
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for subcommand in [
        "telemetry",
        "export",
        "run",
        "recall",
        "score",
        "feedback",
        "quiz",
    ] {
        assert!(
            stdout.contains(subcommand),
            "help 缺少子命令 {subcommand}；实际输出:\n{stdout}"
        );
    }
}

#[test]
fn telemetry_reads_legacy_log_and_writes_report() {
    let dir = temp_dir("telemetry");
    let log = dir.join("context.log");
    let lines = [
        r#"{"ts":"2026-10-01T00:00:00.000000+08:00","session":"s1","msg":"[compact] 开始：messages=10 early=9 early_tokens=100 previous_summary=0 chars mode=llm"}"#,
        r#"{"ts":"2026-10-01T00:00:20.000000+08:00","session":"s1","msg":"[compact] 完成：summary=42 chars recent_messages=2"}"#,
    ];
    std::fs::write(&log, lines.join("\n")).expect("write log");
    let out = dir.join("report.json");

    let output = aemeath()
        .args(["eval", "compact", "telemetry", "--log"])
        .arg(&log)
        .arg("--since")
        .arg("2026-10-01")
        .arg("--output")
        .arg(&out)
        .output()
        .expect("run telemetry");
    assert!(
        output.status.success(),
        "telemetry failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).expect("report")).expect("json");
    assert_eq!(report["total_compactions"], 1);
    assert_eq!(report["duration_ms"]["p50"], 20_000);
    assert!(report["by_model"]["unknown"]["count"].as_u64() == Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn score_counts_missing_summary_as_skipped() {
    let dir = temp_dir("score");
    let cases = dir.join("cases");
    std::fs::create_dir_all(cases.join("c1")).expect("case dir");
    std::fs::write(
        cases.join("cases.jsonl"),
        r#"{"case_id":"c1","message_count":2,"estimated_tokens":100,"must_keep":["a.rs"]}"#,
    )
    .expect("cases index");
    std::fs::write(
        cases.join("c1/messages.jsonl"),
        "{\"role\":\"user\",\"content\":[]}\n",
    )
    .expect("messages");
    let out = dir.join("score.json");

    let output = aemeath()
        .args(["eval", "compact", "score", "--cases"])
        .arg(&cases)
        .arg("--output")
        .arg(&out)
        .output()
        .expect("run score");
    assert!(
        output.status.success(),
        "score failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).expect("score json")).expect("json");
    assert_eq!(report["total"], 1);
    assert_eq!(report["evaluated"], 0);
    assert_eq!(report["skipped"], 1);
    assert!(report["must_keep_recall_mean"].is_null());
    let _ = std::fs::remove_dir_all(&dir);
}
