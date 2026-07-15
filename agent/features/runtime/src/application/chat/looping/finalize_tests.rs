use super::*;
use share::config::hooks::HookEntry;

fn stop_hook_feedback_for_test(
    hook_results: &[(
        share::config::hooks::HookEntry,
        HookResult,
        Option<HookJsonOutput>,
    )],
) -> Option<String> {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(stop_hook_feedback(hook_results, "test-session", "zh"))
}

fn hook_result(
    command: &str,
    blocked: bool,
    output: &str,
    error: Option<&str>,
) -> (
    share::config::hooks::HookEntry,
    HookResult,
    Option<HookJsonOutput>,
) {
    (
        share::config::hooks::HookEntry {
            matcher: String::new(),
            command: command.to_string(),
            timeout: 60,
        },
        HookResult {
            blocked,
            output: output.to_string(),
            error: error.map(str::to_string),
            exit_code: if blocked { Some(2) } else { Some(0) },
        },
        None,
    )
}

#[test]
fn test_stop_hook_feedback_returns_none_without_block() {
    let results = vec![hook_result("echo ok", false, "done", None)];

    assert!(stop_hook_feedback_for_test(&results).is_none());
}

#[test]
fn test_stop_hook_feedback_uses_error_when_blocked() {
    let results = vec![hook_result("check.sh", true, "", Some("failed"))];

    let feedback = stop_hook_feedback_for_test(&results).unwrap();

    assert!(feedback.contains("check.sh"));
    assert!(feedback.contains("failed"));
}

#[test]
fn test_stop_hook_feedback_uses_stdout_when_blocked() {
    let results = vec![hook_result("check.sh", true, "unsafe op found\n", None)];

    let feedback = stop_hook_feedback_for_test(&results).unwrap();

    assert!(feedback.contains("check.sh"));
    assert!(feedback.contains("unsafe op found"));
}

#[test]
fn test_stop_hook_feedback_uses_later_stdout_after_empty_blocked_result() {
    let results = vec![
        hook_result("build.sh", false, "build ok", None),
        hook_result("line-check.sh", true, "", None),
        hook_result("line-check.sh", true, "line limit exceeded", None),
    ];

    let feedback = stop_hook_feedback_for_test(&results).unwrap();

    assert!(feedback.contains("line-check.sh"));
    assert!(feedback.contains("line limit exceeded"));
}

#[test]
fn test_stop_hook_feedback_includes_error_and_stdout_when_blocked() {
    let results = vec![hook_result(
        "check.sh",
        true,
        "stdout details",
        Some("stderr details"),
    )];

    let feedback = stop_hook_feedback_for_test(&results).unwrap();

    assert!(feedback.contains("check.sh"));
    assert!(feedback.contains("stderr/错误"));
    assert!(feedback.contains("stderr details"));
    assert!(feedback.contains("stdout："));
    assert!(feedback.contains("stdout details"));
}

#[test]
fn test_hook_feedback_details_writes_long_output_to_file() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let long_output = "x".repeat(INLINE_HOOK_OUTPUT_LIMIT + 1);
    let result = HookResult {
        blocked: true,
        output: long_output,
        error: Some("stderr details".to_string()),
        exit_code: Some(2),
    };

    let feedback = runtime.block_on(hook_feedback_details(
        &result,
        &None,
        "test-long-output",
        "check long.sh",
        "zh",
    ));

    assert!(feedback.contains("hook 输出过长"));
    assert!(feedback.contains("已保存到文件"));
    assert!(feedback.contains(&std::env::temp_dir().display().to_string()));
}

#[test]
fn test_stop_hook_feedback_uses_json_reason() {
    let results = vec![hook_result_with_json_reason("check.sh", "fix line count")];

    let feedback = stop_hook_feedback_for_test(&results).unwrap();

    assert!(feedback.contains("check.sh"));
    assert!(feedback.contains("fix line count"));
}

#[test]
fn test_stop_hook_feedback_tells_llm_it_must_not_finish() {
    let results = vec![hook_result(
        "check-stop.sh",
        true,
        "fix the failing test",
        Some("exit code 2"),
    )];

    let feedback = stop_hook_feedback_for_test(&results).unwrap();

    assert!(
        feedback.contains("不能结束") || feedback.contains("MUST NOT finish"),
        "feedback must explicitly tell the LLM it cannot finish yet: {feedback}"
    );
    assert!(
        feedback.contains("MUST") || feedback.contains("必须"),
        "feedback must use mandatory language: {feedback}"
    );
    assert!(feedback.contains("check-stop.sh"));
    assert!(feedback.contains("fix the failing test"));
}

fn hook_result_with_json_reason(
    command: &str,
    reason: &str,
) -> (HookEntry, HookResult, Option<HookJsonOutput>) {
    (
        HookEntry {
            matcher: String::new(),
            command: command.to_string(),
            timeout: 60,
        },
        HookResult {
            blocked: false,
            output: String::new(),
            error: None,
            exit_code: Some(0),
        },
        Some(HookJsonOutput {
            decision: Some("block".to_string()),
            reason: Some(reason.to_string()),
            ..Default::default()
        }),
    )
}
