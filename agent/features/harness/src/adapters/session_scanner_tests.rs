//! session scanner 契约测试：真实 dataset 形态 fixture → 事件流提取。

use crate::adapters::session_scanner::SessionDatasetScanner;
use crate::domain::evidence::EvidenceSourceRef;

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
            "accepted_input": {"messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "修复 x.rs 的重复匹配错误"}
                ]}
            ]},
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

/// 项目布局 dataset 的 session id MUST 保留项目段：
/// `project/<session>.dataset` 是 RSI 区分项目/全局落点的最小身份。
#[test]
fn scan_dataset_under_root_preserves_project_scope() {
    let root = tempfile::tempdir().unwrap();
    let dataset = make_dataset(&root.path().join("project-a"));
    let session_key = format!(
        "project-a/{}",
        dataset
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
    );
    let report = SessionDatasetScanner::scan_dataset_under_root(root.path(), &session_key);
    assert!(!report.events.is_empty());
    assert!(report
        .events
        .iter()
        .all(|event| event.session_id == session_key));

    let source = EvidenceSourceRef {
        session_id: session_key.clone(),
        run_id: "run-1".into(),
        step_id: "step-1".into(),
    };
    let excerpt = SessionDatasetScanner::read_step_context_under_root(root.path(), &source)
        .expect("项目布局来源 MUST 可回读");
    assert!(excerpt.text.contains("[step-1]"));
}

/// cursor 身份缺失或不一致时跳过成员，NEVER 生成 unknown 来源绑定错误上下文。
#[test]
fn scan_skips_missing_or_mismatched_step_identity() {
    let root = tempfile::tempdir().unwrap();
    let members = root.path().join("bad.dataset/members");
    std::fs::create_dir_all(&members).unwrap();
    for (name, member) in [
        (
            "missing-cursor.json",
            serde_json::json!({"step":{"outcome":{"messages":[]}}}),
        ),
        (
            "mismatch.json",
            serde_json::json!({
                "cursor": {"run_id":"run-1","step_id":"step-1"},
                "step": {"step_id":"step-2","outcome":{"messages":[]}}
            }),
        ),
    ] {
        std::fs::write(members.join(name), member.to_string()).unwrap();
    }
    let report = SessionDatasetScanner::scan_dataset(&root.path().join("bad.dataset"));
    assert_eq!(report.events.len(), 0);
    assert_eq!(report.skipped_members, 2);
}

/// 回读来源 step 的相邻上下文：包含前后 step、用户/assistant/tool 摘要。
#[test]
fn read_step_context_includes_neighbor_steps_and_tool_facts() {
    let root = tempfile::tempdir().unwrap();
    let dataset = make_dataset(root.path());
    let source = EvidenceSourceRef {
        session_id: "0199abcd-0000-7000-8000-000000000001".into(),
        run_id: "run-1".into(),
        step_id: "step-2".into(),
    };
    let excerpt = SessionDatasetScanner::read_step_context(&dataset, &source)
        .expect("来源 step 上下文必须可回读");
    assert_eq!(excerpt.source, source);
    assert!(
        excerpt.text.contains("[step-1]"),
        "必须包含前一步：{}",
        excerpt.text
    );
    assert!(excerpt.text.contains("[step-2]"));
    assert!(excerpt.text.contains("cargo test"));
    assert!(excerpt.text.contains("修复 x.rs 的重复匹配错误"));
    assert!(excerpt.text.contains("old_string found 2 times"));
    assert!(excerpt.text.chars().count() <= 1200, "单来源上下文必须有界");
}

/// 来源 step 不存在或 run 不匹配时返回 None，NEVER 拼凑无关上下文。
#[test]
fn read_step_context_returns_none_for_missing_source() {
    let root = tempfile::tempdir().unwrap();
    let dataset = make_dataset(root.path());
    let missing_step = EvidenceSourceRef {
        session_id: "0199abcd-0000-7000-8000-000000000001".into(),
        run_id: "run-1".into(),
        step_id: "missing".into(),
    };
    assert!(SessionDatasetScanner::read_step_context(&dataset, &missing_step).is_none());
    let wrong_run = EvidenceSourceRef {
        session_id: "0199abcd-0000-7000-8000-000000000001".into(),
        run_id: "other-run".into(),
        step_id: "step-1".into(),
    };
    assert!(SessionDatasetScanner::read_step_context(&dataset, &wrong_run).is_none());
}

/// 超长 step 上下文按来源预算截断，NEVER 无界注入 prompt。
#[test]
fn read_step_context_is_bounded_for_large_steps() {
    let root = tempfile::tempdir().unwrap();
    let dataset = root.path().join("large.dataset");
    let members = dataset.join("members");
    std::fs::create_dir_all(&members).unwrap();
    for step_index in 0..3 {
        let member = serde_json::json!({
            "cursor": {"run_id": "run-1", "step_id": format!("step-{step_index}")},
            "step": {
                "step_id": format!("step-{step_index}"),
                "outcome": {
                    "committed_revision": step_index,
                    "messages": [
                        {"role": "assistant", "content": [
                            {"type": "text", "text": "长".repeat(2000)}
                        ]}
                    ]
                }
            }
        });
        std::fs::write(
            members.join(format!("m{step_index}.json")),
            member.to_string(),
        )
        .unwrap();
    }
    let source = EvidenceSourceRef {
        session_id: "large.dataset".into(),
        run_id: "run-1".into(),
        step_id: "step-1".into(),
    };
    let excerpt =
        SessionDatasetScanner::read_step_context(&dataset, &source).expect("上下文必须可回读");
    assert!(excerpt.text.chars().count() <= 1200);
    assert!(excerpt.text.contains("[step-1]"), "目标 step MUST 保留");
    assert!(excerpt.text.ends_with('…'));
}

/// datasets root 定位 MUST 拒绝绝对路径、父目录引用与 symlink 逃逸。
#[test]
fn read_step_context_under_root_rejects_path_escape() {
    let root = tempfile::tempdir().unwrap();
    let dataset = make_dataset(root.path());
    let source = EvidenceSourceRef {
        session_id: dataset
            .file_name()
            .and_then(|name| name.to_str())
            .expect("dataset name")
            .to_string(),
        run_id: "run-1".into(),
        step_id: "step-1".into(),
    };
    assert!(
        SessionDatasetScanner::read_step_context_under_root(root.path(), &source).is_some(),
        "合法 dataset 必须可读"
    );

    for session_id in [
        "../escape.dataset",
        "/tmp/escape.dataset",
        "nested/escape.dataset",
    ] {
        let escaped = EvidenceSourceRef {
            session_id: session_id.into(),
            run_id: "run-1".into(),
            step_id: "step-1".into(),
        };
        assert!(
            SessionDatasetScanner::read_step_context_under_root(root.path(), &escaped).is_none(),
            "非法 session id 必须拒绝：{session_id}"
        );
    }

    let outside = tempfile::tempdir().unwrap();
    let outside_dataset = make_dataset(outside.path());
    std::os::unix::fs::symlink(&outside_dataset, root.path().join("symlink.dataset"))
        .expect("symlink");
    let symlink_source = EvidenceSourceRef {
        session_id: "symlink.dataset".into(),
        run_id: "run-1".into(),
        step_id: "step-1".into(),
    };
    assert!(
        SessionDatasetScanner::read_step_context_under_root(root.path(), &symlink_source).is_none(),
        "symlink 指向 root 外 MUST 拒绝"
    );

    let only_link_members = root.path().join("only-link.dataset/members");
    std::fs::create_dir_all(&only_link_members).unwrap();
    std::os::unix::fs::symlink(
        outside_dataset.join("members/m1.json"),
        only_link_members.join("escape.json"),
    )
    .expect("member symlink");
    let member_escape_source = EvidenceSourceRef {
        session_id: "only-link.dataset".into(),
        run_id: "run-1".into(),
        step_id: "step-1".into(),
    };
    assert!(
        SessionDatasetScanner::read_step_context_under_root(root.path(), &member_escape_source)
            .is_none(),
        "成员级 symlink 指向 root 外 MUST 拒绝"
    );
}

/// 同一 step 的多 revision 只保留最新副本作为来源锚点。
#[test]
fn read_step_context_prefers_latest_revision_for_same_step() {
    let root = tempfile::tempdir().unwrap();
    let members = root.path().join("revisions.dataset/members");
    std::fs::create_dir_all(&members).unwrap();
    for (revision, text) in [(0, "旧版本事实"), (2, "新版本事实")] {
        let member = serde_json::json!({
            "cursor": {"run_id": "run-1", "step_id": "step-1"},
            "step": {
                "step_id": "step-1",
                "outcome": {
                    "committed_revision": revision,
                    "messages": [
                        {"role": "assistant", "content": [{"type": "text", "text": text}]}
                    ]
                }
            }
        });
        std::fs::write(
            members.join(format!("rev-{revision}.json")),
            member.to_string(),
        )
        .unwrap();
    }
    let dataset = root.path().join("revisions.dataset");
    let source = EvidenceSourceRef {
        session_id: "revisions.dataset".into(),
        run_id: "run-1".into(),
        step_id: "step-1".into(),
    };
    let excerpt =
        SessionDatasetScanner::read_step_context(&dataset, &source).expect("上下文必须可回读");
    assert!(excerpt.text.contains("新版本事实"));
    assert!(!excerpt.text.contains("旧版本事实"));
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

/// 真 corpus 冒烟：对真实 session dataset 扫描并回读来源上下文。
/// 默认忽略（依赖本机运行数据）；用 AEMEATH_TEST_SESSIONS 指定目录后
/// `cargo test -p harness read_real_session_context_smoke -- --ignored` 运行。
#[test]
#[ignore = "依赖本机真实 session dataset"]
fn read_real_session_context_smoke() {
    let root = std::env::var_os("AEMEATH_TEST_SESSIONS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var("HOME").expect("HOME")).join(".agents/session")
        });
    let dataset = find_dataset_with_members(&root, 2).expect("真实 session dataset");
    let report = SessionDatasetScanner::scan_dataset(&dataset);
    let event = report.events.first().expect("dataset 至少有一条事件");
    let source = EvidenceSourceRef {
        session_id: event.session_id.clone(),
        run_id: event.run_id.clone(),
        step_id: event.step_id.clone(),
    };
    let excerpt = SessionDatasetScanner::read_step_context(&dataset, &source)
        .expect("真实来源必须可回读上下文");
    assert!(!excerpt.text.trim().is_empty());
    assert!(excerpt.text.chars().count() <= 1200);
}

fn find_dataset_with_members(root: &std::path::Path, depth: usize) -> Option<std::path::PathBuf> {
    if depth == 0 {
        return None;
    }
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".dataset"))
            && path
                .join("members")
                .read_dir()
                .is_ok_and(|mut members| members.next().is_some())
        {
            return Some(path);
        }
        if let Some(found) = find_dataset_with_members(&path, depth - 1) {
            return Some(found);
        }
    }
    None
}
