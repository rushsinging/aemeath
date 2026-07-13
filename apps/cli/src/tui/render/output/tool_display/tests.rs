use super::*;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use ratatui::text::Line;

/// 辅助函数：从 Line 中提取纯文本。
fn line_to_string(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn test_lookup_display_finds_task_list_create() {
    let display = lookup_display("TaskListCreate");
    assert!(
        display.is_some(),
        "TaskListCreate 应在 display registry 中注册"
    );
    assert_eq!(display.unwrap().name(), "TaskListCreate");
}

#[test]
fn test_lookup_display_finds_task_create() {
    assert!(lookup_display("TaskCreate").is_some());
}

#[test]
fn test_lookup_display_finds_task_update() {
    assert!(lookup_display("TaskUpdate").is_some());
}

#[test]
fn test_format_tool_call_task_list_create() {
    let (header, details) = format_tool_call(
        "TaskListCreate",
        r#"{"subject":"修复 bug 84","summary":"修复渲染"}"#,
        None,
        None,
    );
    let text = line_to_string(&header);
    assert!(
        text.contains("修复 bug 84"),
        "header 应包含 subject: {text}"
    );
    assert!(
        details.is_empty(),
        "Compact 模式不应显示 details: {details:?}"
    );
}

#[test]
fn test_format_tool_call_task_create_compact_merges_description_into_header() {
    let (header, details) = format_tool_call(
        "TaskCreate",
        r#"{"subject":"分析","description":"查看结构"}"#,
        None,
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("分析"), "header: {text}");
    assert!(
        text.contains("查看结构"),
        "header 应合并 description: {text}"
    );
    assert!(
        details.is_empty(),
        "Compact 模式不应显示 details: {details:?}"
    );
}

#[test]
fn test_format_tool_call_task_create_compact_no_description() {
    let (header, details) = format_tool_call("TaskCreate", r#"{"subject":"分析"}"#, None, None);
    let text = line_to_string(&header);
    assert!(text.contains("分析"), "header: {text}");
    assert!(
        !text.contains(':'),
        "无 description 时 header 不应有冒号: {text}"
    );
    assert!(details.is_empty());
}

#[test]
fn test_format_tool_call_task_update_compact_hides_details() {
    let (header, details) = format_tool_call(
        "TaskUpdate",
        r#"{"taskId":"42","status":"completed"}"#,
        None,
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("42"), "header 应包含 taskId: {text}");
    assert!(text.contains("completed"), "header 应包含 status: {text}");
    assert!(
        details.is_empty(),
        "Compact 模式不应显示 details: {details:?}"
    );
}

#[test]
fn test_format_tool_call_uses_display_name_in_header() {
    // Bash → Run
    let (header, _) = format_tool_call("Bash", r#"{"command":"ls"}"#, None, None);
    let text = line_to_string(&header);
    assert!(
        text.starts_with("Run "),
        "Bash header 应使用 display name 'Run': {text}"
    );

    // Grep → Search
    let (header, _) = format_tool_call("Grep", r#"{"pattern":"foo"}"#, None, None);
    let text = line_to_string(&header);
    assert!(
        text.starts_with("Search "),
        "Grep header 应使用 display name 'Search': {text}"
    );

    // Glob → Find
    let (header, _) = format_tool_call("Glob", r#"{"pattern":"*.rs"}"#, None, None);
    let text = line_to_string(&header);
    assert!(
        text.starts_with("Find "),
        "Glob header 应使用 display name 'Find': {text}"
    );
}

#[test]
fn test_format_tool_call_unknown_tool_uses_fallback() {
    let (header, details) = format_tool_call("UnknownTool", r#"{"key":"value"}"#, None, None);
    let text = line_to_string(&header);
    assert_eq!(text, "● UnknownTool");
    assert!(!details.is_empty(), "fallback 应截断 JSON");
}

#[test]
fn test_format_tool_call_invalid_json_uses_fallback() {
    let (header, _details) = format_tool_call("TaskListCreate", "not json", None, None);
    // 不应 panic，应 fallback。display name 为 "New Task List"。
    let text = line_to_string(&header);
    assert!(text.contains("New Task List"));
}

#[test]
fn test_result_render_kind_diff_only_for_edit() {
    // 渲染类型由工具声明：Edit→Diff，其它工具与未注册工具→Plain（防 ---DIFF--- 误判）。
    assert_eq!(result_render_kind("Edit"), ResultRender::Diff);
    assert_eq!(result_render_kind("Read"), ResultRender::Plain);
    assert_eq!(result_render_kind("UnknownTool"), ResultRender::Plain);
}

#[test]
fn test_result_policy_read_is_hidden() {
    assert_eq!(
        result_policy("Read"),
        ResultPolicy::Hidden,
        "Read 的 result 策略应为 Hidden"
    );
}

#[test]
fn test_result_policy_bash_is_tail_mode() {
    assert_eq!(
        result_policy("Bash"),
        ResultPolicy::Visible {
            max_lines: Some(5),
            render_kind: ResultRender::Plain,
            tail_mode: true,
        },
        "Bash 的 result 策略应为 tail 模式"
    );
}

#[test]
fn test_format_tool_call_bash_long_cjk_command_no_panic() {
    // 回归 #218：含中文的超长 Bash 命令按字节切片会落在多字节字符内部触发 panic。
    let cmd = "gh pr create --title 'fix(runtime): 使用人类可读摘要替代 JSON 作为 tool call summary' --body '## 问题'";
    let raw = serde_json::json!({ "command": cmd }).to_string();
    let (header, _details) = format_tool_call("Bash", &raw, None, None);
    let text = line_to_string(&header);
    assert!(
        text.starts_with("Run "),
        "header 应以 'Run ' 开头 (Bash display name): {text}"
    );
    assert!(
        text.ends_with("..."),
        "超长命令应被截断并以 ... 结尾: {text}"
    );
}

#[test]
fn test_format_tool_call_read_long_cjk_path_no_panic() {
    // 回归 #218：含中文的超长路径经 truncate_path 尾部截断不应 panic。
    let path = format!("/项目/{}/数据/报告为准的文件名.rs", "子目录".repeat(20));
    let raw = serde_json::json!({ "file_path": path }).to_string();
    let (header, _details) = format_tool_call("Read", &raw, None, None);
    let text = line_to_string(&header);
    assert!(
        text.starts_with("Read "),
        "header 应以 'Read ' 开头: {text}"
    );
    assert!(text.contains("..."), "超长路径应被截断: {text}");
}

// ── 回归 #304 ──────────────────────────────────────────────────
// 参数未就绪时 header 不应显示 "?" 占位（参数为空时只显示工具名）。
// 覆盖 task_impls.rs 与 tool_impls.rs 中所有 ? → "" 的修复点。

#[test]
fn test_format_tool_call_bash_empty_command_no_question_mark() {
    let (header, _details) = format_tool_call("Bash", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "Run",
        "空 command 时应只显示 display name 'Run': {text}"
    );
    assert!(!text.contains('?'), "header 不应含 '?': {text}");
}

#[test]
fn test_format_tool_call_glob_empty_pattern_no_question_mark() {
    let (header, _details) = format_tool_call("Glob", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "Find",
        "空 pattern 时应只显示 display name 'Find': {text}"
    );
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_grep_empty_pattern_no_question_mark() {
    let (header, _details) = format_tool_call("Grep", r#"{"path":"."}"#, None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "Search in .",
        "空 pattern 时应显示 display name 'Search in .': {text}"
    );
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_task_create_empty_subject_no_question_mark() {
    let (header, _details) = format_tool_call("TaskCreate", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "Task",
        "空 subject 时应只显示 display name 'Task': {text}"
    );
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_task_create_with_subject_and_description() {
    // 回归：subject + description 都给定时，header 应保留 ":" 分隔与截断。
    let (header, _details) = format_tool_call(
        "TaskCreate",
        r#"{"subject":"分析","description":"查看结构"}"#,
        None,
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("分析"), "header: {text}");
    assert!(text.contains("查看结构"), "header: {text}");
}

#[test]
fn test_format_tool_call_task_update_empty_id_no_question_mark() {
    let (header, _details) = format_tool_call("TaskUpdate", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "Task",
        "空 taskId 时应只显示 display name 'Task': {text}"
    );
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_task_update_with_id_no_status() {
    // 回归：id 给定但 status 缺失时，header 应只显示 id。
    let (header, _details) = format_tool_call("TaskUpdate", r#"{"taskId":"42"}"#, None, None);
    let text = line_to_string(&header);
    assert!(text.contains("42"));
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_task_update_shows_blocked_by() {
    let (header, _) = format_tool_call(
        "TaskUpdate",
        r#"{"taskId":"7","addBlockedBy":["4","5","6"]}"#,
        None,
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("7"), "应包含 taskId: {text}");
    assert!(
        text.contains("blocked by [4,5,6]"),
        "应包含 blockedBy: {text}"
    );
}

#[test]
fn test_format_tool_call_task_update_shows_status_and_blocked_by() {
    let (header, _) = format_tool_call(
        "TaskUpdate",
        r#"{"taskId":"2","status":"InProgress","addBlockedBy":["1"]}"#,
        None,
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("→ InProgress"), "应包含 status: {text}");
    assert!(text.contains("blocked by [1]"), "应包含 blockedBy: {text}");
}

// ── Issue #486：TaskUpdate result 到达后 header 应带 task 标题 ──────

#[test]
fn test_format_tool_call_task_update_with_result_subject_shows_title() {
    use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
    // LLM 只传 taskId + status（典型场景），subject 由 store 回填到 typed result
    let payload = ToolResultPayload::new(
        String::new(),
        serde_json::json!({ "task_id": "2", "status": "Completed", "subject": "修复渲染 bug" }),
        false,
        0,
    );
    let (header, _) = format_tool_call(
        "TaskUpdate",
        r#"{"taskId":"2","status":"completed"}"#,
        Some(&payload),
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("2"), "应包含 taskId: {text}");
    assert!(
        text.contains("修复渲染 bug"),
        "result 到达后 header 应包含 subject: {text}"
    );
    assert!(text.contains("→ completed"), "应包含 status: {text}");
}

#[test]
fn test_format_tool_call_task_update_result_falls_back_to_input_subject() {
    // result 无 typed subject 时回退到 input.subject
    let payload = ToolResultPayload::new(
        String::new(),
        serde_json::json!({ "task_id": "3", "status": "InProgress" }),
        false,
        0,
    );
    let (header, _) = format_tool_call(
        "TaskUpdate",
        r#"{"taskId":"3","subject":"重构模块"}"#,
        Some(&payload),
        None,
    );
    let text = line_to_string(&header);
    assert!(
        text.contains("重构模块"),
        "typed subject 缺失时应回退 input.subject: {text}"
    );
}

#[test]
fn test_format_tool_call_task_update_no_subject_anywhere_omits_title() {
    // 既无 typed subject 也无 input.subject → 不显示标题占位（向后兼容）
    let payload = ToolResultPayload::new(
        String::new(),
        serde_json::json!({ "task_id": "4", "status": "Completed" }),
        false,
        0,
    );
    let (header, _) = format_tool_call(
        "TaskUpdate",
        r#"{"taskId":"4","status":"completed"}"#,
        Some(&payload),
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("→ completed"), "应包含 status: {text}");
    // 无标题时仍是合法格式 `Task 4 — → completed`，不 crash
    assert!(!text.contains("— ,"), "不应出现空 parts 分隔符: {text}");
}

#[test]
fn test_format_tool_call_task_get_empty_id_no_question_mark() {
    let (header, _details) = format_tool_call("TaskGet", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "Task",
        "空 taskId 时应只显示 display name 'Task': {text}"
    );
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_task_stop_empty_id_no_question_mark() {
    let (header, _details) = format_tool_call("TaskStop", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "Stop Task",
        "空 taskId 时应只显示 display name 'Stop Task': {text}"
    );
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_task_list_create_empty_subject_no_question_mark() {
    let (header, _details) = format_tool_call("TaskListCreate", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "New Task List",
        "空 subject 时应只显示 display name 'New Task List': {text}"
    );
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_skill_empty_no_question_mark() {
    let (header, _details) = format_tool_call("Skill", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(text, "Skill");
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_lsp_both_empty_no_question_mark() {
    let (header, _details) = format_tool_call("LSP", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(text, "LSP");
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_lsp_only_operation_no_path() {
    let (header, _details) = format_tool_call("LSP", r#"{"operation":"hover"}"#, None, None);
    let text = line_to_string(&header);
    assert_eq!(text, "LSP::hover");
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_lsp_only_path_no_operation() {
    let (header, _details) = format_tool_call("LSP", r#"{"filePath":"/tmp/x.rs"}"#, None, None);
    let text = line_to_string(&header);
    assert_eq!(text, "LSP /tmp/x.rs");
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_lsp_full_no_question_mark() {
    let (header, _details) = format_tool_call(
        "LSP",
        r#"{"operation":"hover","filePath":"/tmp/x.rs"}"#,
        None,
        None,
    );
    let text = line_to_string(&header);
    assert_eq!(text, "LSP::hover /tmp/x.rs");
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_web_fetch_empty_url_no_question_mark() {
    let (header, _details) = format_tool_call("WebFetch", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(text, "WebFetch");
    assert!(!text.contains('?'));
}

#[test]
fn test_format_tool_call_ask_user_question_empty_no_question_mark() {
    let (header, _details) = format_tool_call("AskUserQuestion", "{}", None, None);
    let text = line_to_string(&header);
    assert_eq!(
        text, "Ask",
        "空 question 时应只显示 display name 'Ask': {text}"
    );
    assert!(!text.contains('?'));
}

// ── 回归 #273 typed payload 路径 ──────────────────────────────────
// Read/Write header 在 result 到达后应优先使用 typed data 字段（issue #273
// 引入的 typed R 路径），不再依赖 regex 解析 message 文本。覆盖：
// - payload.content.data.line_count (Read)
// - payload.content.data.bytes_written (Write)
// 旧 regex 路径通过 fallback 行为继续兼容（payload.output 含 "Read N lines from"）。

#[test]
fn test_format_tool_call_read_uses_typed_line_count_from_payload() {
    use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
    // typed 路径：payload.content.line_count=340 应直接驱动 header 后缀
    let payload = ToolResultPayload::new(
        String::new(), // output 不参与 typed 解析
        serde_json::json!({ "content": "", "file_path": "/src/lib.rs", "line_count": 340u64, "start_line": 1u64, "total_lines": 500u64 }),
        false,
        0,
    );
    let (header, _details) = format_tool_call(
        "Read",
        r#"{"file_path":"/src/lib.rs","offset":0,"limit":2000}"#,
        Some(&payload),
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("Read "), "header: {text}");
    assert!(
        text.contains("1:340"),
        "header 应使用 typed line_count=340 覆盖默认 limit: {text}"
    );
    assert!(
        text.contains("(340 lines)"),
        "header 应包含 typed 行数后缀: {text}"
    );
}

#[test]
fn test_format_tool_call_write_uses_typed_bytes_written_from_payload() {
    use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
    // typed 路径：payload.content.bytes_written=1234 应直接驱动 header 字节数
    let payload = ToolResultPayload::new(
        String::new(),
        serde_json::json!({ "file_path": "/tmp/x.rs", "bytes_written": 1234u64 }),
        false,
        0,
    );
    let (header, _details) = format_tool_call(
        "Write",
        r#"{"file_path":"/tmp/x.rs","content":"abc"}"#,
        Some(&payload),
        None,
    );
    let text = line_to_string(&header);
    assert!(text.contains("Write "), "header: {text}");
    assert!(
        text.contains("1234 bytes"),
        "header 应使用 typed bytes_written=1234: {text}"
    );
}

#[test]
fn test_format_tool_call_read_falls_back_to_regex_when_typed_missing() {
    // 旧 ToolResult data 无 typed 字段（仅 output 文本 "Read N lines from ..."），
    // regex 路径应继续生效。
    use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
    let payload = ToolResultPayload::new(
        "Read 200 lines from /src/legacy.rs".to_string(),
        serde_json::Value::Null, // 模拟旧格式，无 data.typed 字段
        false,
        0,
    );
    let (header, _details) = format_tool_call(
        "Read",
        r#"{"file_path":"/src/legacy.rs","offset":0,"limit":2000}"#,
        Some(&payload),
        None,
    );
    let text = line_to_string(&header);
    assert!(
        text.contains("(200 lines)"),
        "typed 缺失时 regex fallback 应生效: {text}"
    );
}

// ── Issue #342：workspace_root 路径相对化端到端 ──────────────────

#[test]
fn test_format_tool_call_read_path_relativized_with_workspace_root() {
    let root = std::path::Path::new("/home/user/project");
    let raw = serde_json::json!({
        "file_path": "/home/user/project/src/main.rs",
        "offset": 0,
        "limit": 100
    })
    .to_string();
    let (header, _) = format_tool_call("Read", &raw, None, Some(root));
    let text = line_to_string(&header);
    assert!(
        text.contains("src/main.rs"),
        "header 应显示相对路径 'src/main.rs'，实际: {text}"
    );
    assert!(
        !text.contains("/home/user/project/src/main.rs"),
        "header 不应包含绝对路径: {text}"
    );
}

#[test]
fn test_format_tool_call_read_path_no_workspace_root_shows_raw() {
    // workspace_root 为 None 时应显示原始路径（不相对化）
    let raw = serde_json::json!({
        "file_path": "/home/user/project/src/main.rs",
        "offset": 0,
        "limit": 100
    })
    .to_string();
    let (header, _) = format_tool_call("Read", &raw, None, None);
    let text = line_to_string(&header);
    assert!(
        text.contains("/home/user/project/src/main.rs"),
        "workspace_root=None 时应显示原始绝对路径: {text}"
    );
}

#[test]
fn test_format_tool_call_write_path_relativized() {
    let root = std::path::Path::new("/work/repo");
    let raw =
        serde_json::json!({"file_path": "/work/repo/lib/mod.rs", "content": "hello"}).to_string();
    let (header, _) = format_tool_call("Write", &raw, None, Some(root));
    let text = line_to_string(&header);
    assert!(
        text.contains("lib/mod.rs"),
        "Write header 应显示相对路径: {text}"
    );
}

#[test]
fn test_format_tool_call_edit_path_relativized() {
    let root = std::path::Path::new("/work/repo");
    let raw = serde_json::json!({
        "file_path": "/work/repo/src/lib.rs",
        "old_string": "a",
        "new_string": "b"
    })
    .to_string();
    let (header, _) = format_tool_call("Edit", &raw, None, Some(root));
    let text = line_to_string(&header);
    assert!(
        text.contains("src/lib.rs"),
        "Edit header 应显示相对路径: {text}"
    );
}

#[test]
fn test_format_tool_call_grep_path_relativized() {
    let root = std::path::Path::new("/work/repo");
    let raw = serde_json::json!({"pattern": "foo", "path": "/work/repo/src"}).to_string();
    let (header, _) = format_tool_call("Grep", &raw, None, Some(root));
    let text = line_to_string(&header);
    assert!(text.contains("src"), "Grep header 应显示相对路径: {text}");
    assert!(
        !text.contains("/work/repo/src"),
        "Grep header 不应包含绝对路径: {text}"
    );
}

// ── issue #839：snake_case task_id 应正确渲染（alias 生效） ──

#[test]
fn test_task_update_snake_case_task_id_shows_id() {
    // build.rs 生成的 schema 暴露 snake_case，LLM 发 task_id 而非 taskId。
    // 旧 str_arg(input, "taskId", "") 取不到 → 空值 → 裸 display name。
    // 反序列化后 serde alias 自动接受两种 key。
    let raw = serde_json::json!({
        "task_id": "42",
        "status": "completed"
    })
    .to_string();
    let (header, _) = format_tool_call("TaskUpdate", &raw, None, None);
    let text = line_to_string(&header);
    assert!(
        text.contains("42"),
        "TaskUpdate header 应包含 task_id '42'，实际: {text}"
    );
    assert_ne!(text, "● Task", "header 不应退化为裸 display name: {text}");
}

#[test]
fn test_task_update_camel_case_task_id_shows_id() {
    // 旧路径只查 camelCase taskId；确保不回归。
    let raw = serde_json::json!({
        "taskId": "99",
        "status": "in_progress"
    })
    .to_string();
    let (header, _) = format_tool_call("TaskUpdate", &raw, None, None);
    let text = line_to_string(&header);
    assert!(
        text.contains("99"),
        "TaskUpdate header 应包含 taskId '99'，实际: {text}"
    );
}

#[test]
fn test_task_get_snake_case_task_id_shows_id() {
    let raw = serde_json::json!({"task_id": "7"}).to_string();
    let (header, _) = format_tool_call("TaskGet", &raw, None, None);
    let text = line_to_string(&header);
    assert!(
        text.contains("7"),
        "TaskGet header 应包含 task_id '7'，实际: {text}"
    );
}

#[test]
fn test_lsp_snake_case_file_path_shows_path() {
    // LSP Input 有 #[serde(alias = "filePath")]，snake_case file_path 也应生效。
    let raw = serde_json::json!({
        "operation": "diagnostics",
        "file_path": "/repo/src/lib.rs"
    })
    .to_string();
    let (header, _) = format_tool_call("LSP", &raw, None, None);
    let text = line_to_string(&header);
    assert!(
        text.contains("lib.rs"),
        "LSP header 应包含 file_path，实际: {text}"
    );
}
