use super::*;
use std::path::{Path, PathBuf};

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "aemeath-eval-dataset-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temp root");
    root
}

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("member dir");
    std::fs::write(path, serde_json::to_string(value).expect("json")).expect("write member");
}

/// 构造最小 dataset：metadata + session-state + 一个 committed step。
fn build_dataset(root: &Path) -> PathBuf {
    let dataset = root.join("proj").join("sess-1.dataset");
    let members = dataset.join("members");
    std::fs::create_dir_all(&members).expect("members");

    let step_name = step_member_name("run_abc", "stp_def");
    let members_payload = [
        (
            "metadata.json",
            serde_json::json!({
                "id": "sess-1",
                "metadata": { "model": "Zhipu/glm-5.3" }
            }),
        ),
        (
            "session-state.json",
            serde_json::json!({
                "id": "sess-1",
                "compact": { "summary": "之前的摘要" },
                "committed_steps": [
                    { "run_id": "run_abc", "step_id": "stp_def" },
                    { "run_id": "run_missing", "step_id": "stp_missing" }
                ]
            }),
        ),
        (
            "step-placeholder",
            serde_json::json!({
                "cursor": { "run_id": "run_abc", "step_id": "stp_def" },
                "step": {
                    "step_id": "stp_def",
                    "outcome": {
                        "finalize_cause": "Completed",
                        "messages": [
                            { "role": "user", "content": [{ "type": "text", "text": "第一问" }] },
                            { "role": "assistant", "content": [{ "type": "tool_use", "id": "c1", "name": "Grep", "input": { "pattern": "compact" } }] }
                        ]
                    }
                }
            }),
        ),
    ];

    let evidence = vec![
        serde_json::json!({ "名称": "manifest.json", "内容摘要": "manifest" }),
        serde_json::json!({ "名称": "metadata.json", "内容摘要": "metadata" }),
        serde_json::json!({ "名称": "session-state.json", "内容摘要": "state" }),
        serde_json::json!({ "名称": step_name, "内容摘要": "step" }),
    ];
    for (name, payload) in members_payload {
        let digest = match name {
            "metadata.json" => "metadata",
            "session-state.json" => "state",
            _ => "step",
        };
        write_json(&members.join(digest), &payload);
    }
    let manifest = serde_json::json!({ "成员证据": evidence });
    write_json(&dataset.join("primary/manifest.json"), &manifest);
    dataset
}

#[test]
fn step_member_name_uses_hex_encoding() {
    assert_eq!(
        step_member_name("run_abc", "stp_def"),
        "step-72756e5f616263-7374705f646566.json"
    );
}

#[test]
fn estimate_tokens_uses_cjk_and_ascii_weights() {
    assert_eq!(estimate_tokens("中文"), 2);
    assert_eq!(estimate_tokens("abcd"), 2);
    assert_eq!(estimate_tokens(""), 0);
}

#[test]
fn message_text_flattens_text_thinking_tool_use_and_result() {
    let message = serde_json::json!({
        "role": "assistant",
        "content": [
            { "type": "thinking", "thinking": "想一下" },
            { "type": "text", "text": "回答" },
            { "type": "tool_use", "name": "Read", "input": { "file_path": "a.rs" } },
            { "type": "tool_result", "text": "文件内容" }
        ]
    });
    let text = message_text(&message);
    assert!(text.contains("想一下"));
    assert!(text.contains("回答"));
    assert!(text.contains("a.rs"));
    assert!(text.contains("文件内容"));
}

#[test]
fn load_session_reads_steps_in_committed_order_and_skips_missing() {
    let root = temp_root("load");
    let dataset = build_dataset(&root);
    let session = load_session(&dataset).expect("load session");
    assert_eq!(session.session_id, "sess-1");
    assert_eq!(session.model.as_deref(), Some("Zhipu/glm-5.3"));
    assert_eq!(session.previous_summary.as_deref(), Some("之前的摘要"));
    assert_eq!(session.committed_step_count, 2);
    // 缺失的 step 被跳过，只剩第一条的两条消息。
    assert_eq!(session.messages.len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn iter_datasets_finds_only_directories_with_manifest() {
    let root = temp_root("iter");
    build_dataset(&root);
    let datasets = iter_datasets(&root);
    assert_eq!(datasets.len(), 1);
    assert!(datasets[0].ends_with("sess-1.dataset"));
    let _ = std::fs::remove_dir_all(&root);
}
