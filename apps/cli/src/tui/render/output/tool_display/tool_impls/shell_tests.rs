use super::*;

#[test]
fn header_shows_goal_when_present() {
    let display = BashDisplay;
    let input = serde_json::json!({
        "goal": "运行测试",
        "command": "cargo test -- --nocapture"
    });
    let header = display.format_header(&input, None);
    assert!(
        header.contains("运行测试"),
        "header 应包含 goal，实际: {header}"
    );
    // header 不含命令全文
    assert!(
        !header.contains("cargo test"),
        "header 不应包含命令全文，实际: {header}"
    );
}

#[test]
fn header_falls_back_to_command_when_goal_empty() {
    let display = BashDisplay;
    let input = serde_json::json!({
        "goal": "",
        "command": "cargo build"
    });
    let header = display.format_header(&input, None);
    // goal 为空时 fallback：header 显示截断的 command
    assert!(
        header.contains("cargo build"),
        "goal 为空时 header 应 fallback 显示 command，实际: {header}"
    );
}

#[test]
fn details_show_full_command_untruncated() {
    let display = BashDisplay;
    let long_command = "echo hello world && ".repeat(20);
    let input = serde_json::json!({
        "goal": "测试长命令",
        "command": long_command
    });
    let details = display.format_details(&input);
    assert_eq!(details.len(), 1, "details 应只有一行命令全文");
    // 命令全文在 details 中，不截断
    assert_eq!(details[0], long_command);
}

#[test]
fn format_header_line_with_result_uses_goal() {
    let display = BashDisplay;
    let input = serde_json::json!({
        "goal": "构建项目",
        "command": "cargo build --release"
    });
    let line = display.format_header_line_with_result(&input, None, None);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains("构建项目"),
        "header line 应包含 goal，实际: {text}"
    );
    assert!(
        !text.contains("cargo build --release"),
        "header line 不应包含命令全文，实际: {text}"
    );
}

#[test]
fn format_header_line_with_result_shows_exit_suffix() {
    let display = BashDisplay;
    let input = serde_json::json!({
        "goal": "失败的命令",
        "command": "false"
    });
    // 模拟 exit_code=1 的 BashResult
    let result_content = serde_json::json!({
        "stdout": "",
        "stderr": "",
        "exit_code": 1,
        "signal": null,
        "path_base": null,
    });
    let payload = ToolResultPayload::new(String::new(), result_content, true, 0);
    let line = display.format_header_line_with_result(&input, Some(&payload), None);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains("(exit 1)"),
        "header line 应包含 exit suffix，实际: {text}"
    );
    assert!(
        text.contains("失败的命令"),
        "header line 应包含 goal，实际: {text}"
    );
}

#[test]
fn format_header_line_with_result_falls_back_when_goal_empty() {
    let display = BashDisplay;
    let input = serde_json::json!({
        "goal": "",
        "command": "ls -la"
    });
    let line = display.format_header_line_with_result(&input, None, None);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    // goal 为空时 fallback 到 command
    assert!(
        text.contains("ls -la"),
        "goal 为空时 header line 应 fallback 到 command，实际: {text}"
    );
}

#[test]
fn header_for_subagent_uses_goal() {
    let display = BashDisplay;
    let input = serde_json::json!({
        "goal": "子代理任务",
        "command": "echo test"
    });
    let header = display.header_for_subagent(&input, None);
    assert!(
        header.contains("子代理任务"),
        "subagent header 应包含 goal，实际: {header}"
    );
}
