//! session scanner 契约测试：真实 dataset 形态 fixture → 事件流提取。

use crate::adapters::session_scanner::SessionDatasetScanner;

/// 构造一个 dataset 目录（含两条 step 成员 + 一条非 step 成员 + 一条损坏成员）。
fn make_dataset(root: &std::path::Path) -> std::path::PathBuf {
    let dataset = root.join("0199abcd-0000-7000-8000-000000000001.dataset");
    let members = dataset.join("members");
    std::fs::create_dir_all(&members).unwrap();

    // step 1：一次 Bash 成功调用 + finalize_cause
    let step1 = serde_json::json!({
        "cursor": {"run_id": "run-1", "step_id": "step-1"},
        "step": {
            "step_id": "step-1",
            "accepted_input": {"messages": []},
            "outcome": {
                "finalize_cause": "Completed",
                "messages": [
                    {"role": "assistant", "content": [
                        {"type": "tool_use", "id": "call_a", "name": "Bash",
                         "input": {"command": "cargo test", "goal": "g"}}
                    ]},
                    {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": "call_a",
                         "is_error": false, "text": "test result: ok"}
                    ]}
                ]
            }
        }
    });
    std::fs::write(members.join("m1.json"), step1.to_string()).unwrap();

    // step 2：一次 Edit 失败（old_string 不匹配）+ 同参数重试
    let failing_result = serde_json::json!({
        "cursor": {"run_id": "run-1", "step_id": "step-2"},
        "step": {
            "step_id": "step-2",
            "accepted_input": {"messages": []},
            "outcome": {
                "finalize_cause": "Completed",
                "messages": [
                    {"role": "assistant", "content": [
                        {"type": "tool_use", "id": "call_b", "name": "Edit",
                         "input": {"file_path": "x.rs", "old_string": "a", "new_string": "b"}},
                        {"type": "tool_use", "id": "call_c", "name": "Edit",
                         "input": {"file_path": "x.rs", "old_string": "a", "new_string": "b"}}
                    ]},
                    {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": "call_b",
                         "is_error": true, "text": "old_string found 2 times"},
                        {"type": "tool_result", "tool_use_id": "call_c",
                         "is_error": true, "text": "old_string found 2 times"}
                    ]}
                ]
            }
        }
    });
    std::fs::write(members.join("m2.json"), failing_result.to_string()).unwrap();

    // 非 step 成员（metadata）与损坏成员
    std::fs::write(
        members.join("m3.json"),
        serde_json::json!({"session_id": "x", "revision": 3}).to_string(),
    )
    .unwrap();
    std::fs::write(members.join("m4.json"), "not json").unwrap();

    dataset
}

/// 提取：工具配对事件 + run 终态事件；损坏成员跳过计数；非 step 成员忽略。
#[test]
fn scan_extracts_paired_tool_events_and_outcomes() {
    let root = tempfile::tempdir().unwrap();
    let dataset = make_dataset(root.path());
    let report = SessionDatasetScanner::scan_dataset(&dataset);

    // 工具事件：Bash×1 成功 + Edit×2 失败；run 级：Completed×2
    let tool_events: Vec<_> = report
        .events
        .iter()
        .filter(|event| event.tool_name.is_some())
        .collect();
    assert_eq!(tool_events.len(), 3);
    assert_eq!(report.skipped_members, 1, "损坏成员计数");
    assert!(tool_events
        .iter()
        .any(|event| event.tool_name.as_deref() == Some("Bash")
            && !event.is_error
            && event.result_chars > 0));
    let edit_failures: Vec<_> = tool_events
        .iter()
        .filter(|event| event.tool_name.as_deref() == Some("Edit") && event.is_error)
        .collect();
    assert_eq!(edit_failures.len(), 2);
    assert!(edit_failures[0]
        .error_text
        .as_deref()
        .unwrap()
        .contains("old_string"));
    // 两次 Edit 同参数 → 指纹一致（retry_pattern 原料）
    assert_eq!(
        edit_failures[0].call_fingerprint,
        edit_failures[1].call_fingerprint
    );

    let run_events: Vec<_> = report
        .events
        .iter()
        .filter(|event| event.tool_name.is_none())
        .collect();
    assert_eq!(run_events.len(), 2);
    assert!(run_events
        .iter()
        .all(|event| event.error_text.as_deref() == Some("Completed")));
    assert!(report
        .events
        .iter()
        .all(|event| event.session_id.contains("0199abcd") && event.run_id == "run-1"));
}

/// 缺 members 目录：空报告 NEVER 报错。
#[test]
fn scan_missing_members_dir_yields_empty() {
    let root = tempfile::tempdir().unwrap();
    let report = SessionDatasetScanner::scan_dataset(&root.path().join("absent.dataset"));
    assert_eq!(report.events.len(), 0);
    assert_eq!(report.skipped_members, 0);
}

/// 端到端：scanner → aggregate_evidence 产出 repeated_failure（Edit ×2 同签名）。
#[test]
fn scan_then_aggregate_yields_repeated_failure() {
    let root = tempfile::tempdir().unwrap();
    let dataset = make_dataset(root.path());
    let report = SessionDatasetScanner::scan_dataset(&dataset);
    let bundle = crate::domain::evidence::aggregate_evidence(&report.events);

    assert!(
        bundle.signals.iter().any(|signal| {
            signal.kind == crate::domain::evidence::SignalKind::RepeatedFailure
                && signal.summary.contains("Edit")
                && signal.summary.contains("×2")
        }),
        "同签名 Edit 失败必须聚合为 repeated_failure"
    );
    assert!(bundle
        .signals
        .iter()
        .any(|signal| signal.kind == crate::domain::evidence::SignalKind::RetryPattern));
}
