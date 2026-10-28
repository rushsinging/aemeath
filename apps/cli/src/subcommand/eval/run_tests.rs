use super::*;

fn write(dir: &std::path::Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write fixture");
}

#[test]
fn loads_cases_index_and_skips_blank_lines() {
    let dir = tempfile_dir("run-index");
    write(
        &dir,
        "cases.jsonl",
        "{\"case_id\":\"c1\",\"messages_path\":\"c1/messages.jsonl\"}\n\n{\"case_id\":\"c2\",\"messages_path\":\"c2/messages.jsonl\"}\n",
    );
    let index = load_cases_index(&dir).expect("index");
    assert_eq!(index.len(), 2);
    assert_eq!(index[0]["case_id"], "c1");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn loads_message_jsonl_as_values() {
    let dir = tempfile_dir("run-messages");
    write(
        &dir,
        "messages.jsonl",
        "{\"role\":\"user\",\"content\":[]}\n{\"role\":\"assistant\",\"content\":[]}\n",
    );
    let messages = load_case_json_lines(&dir.join("messages.jsonl")).expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn report_counts_succeeded_and_failed() {
    let mut report = RunReport {
        cases_total: 2,
        succeeded: 1,
        failed: 1,
        ..Default::default()
    };
    report.cases.push(CaseRun {
        case_id: "c1".to_string(),
        status: "succeeded".to_string(),
        message_count: 10,
        summary_chars: Some(100),
        recent_messages: Some(5),
        quality: Some("Llm".to_string()),
        elapsed_ms: Some(1000),
        error: None,
    });
    assert_eq!(report.cases_total, 2);
    assert_eq!(report.cases.len(), 1);
}

fn tempfile_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aemeath-eval-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}
