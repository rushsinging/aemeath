//! #1964 Task 5 L0：所有内置工具 `*Input` 的 schema 必须把 `goal` 列为必填。
//!
//! 设计见 `docs/design/02-modules/tui/06-tool-goal-and-background-summary.md` §5.1：
//! 每个内置 `*Input` MUST 含 `pub goal: String`（必填；空串由 adapter 拒绝）。
//! 本测试是「缺一不可」清单的单一真相：新增内置工具时同步登记。

use super::*;

/// 断言某个 Input 的 data_schema（build.rs 生成）把 `goal` 标为 required。
fn assert_goal_required<S: ToolSchema>(tool: &str) {
    let schema = S::data_schema();
    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("{tool} schema 缺少 required 数组"));
    let required_names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        required_names.contains(&"goal"),
        "{tool} 的 schema required 必须包含 goal，实际: {required_names:?}"
    );
    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .unwrap_or_else(|| panic!("{tool} schema 缺少 properties"));
    assert!(
        properties.contains_key("goal"),
        "{tool} 的 schema properties 必须包含 goal"
    );
}

#[test]
fn every_builtin_input_schema_requires_goal() {
    // 核心工具
    assert_goal_required::<BashInput>("Bash");
    assert_goal_required::<GlobInput>("Glob");
    assert_goal_required::<GrepInput>("Grep");
    assert_goal_required::<ReadInput>("Read");
    assert_goal_required::<WriteInput>("Write");
    assert_goal_required::<EditInput>("Edit");
    assert_goal_required::<AgentInput>("Agent");
    assert_goal_required::<WebFetchInput>("WebFetch");
    assert_goal_required::<WebSearchInput>("WebSearch");
    assert_goal_required::<ToolSearchInput>("ToolSearch");
    assert_goal_required::<EnterWorktreeInput>("EnterWorktree");
    assert_goal_required::<ExitWorktreeInput>("ExitWorktree");
    assert_goal_required::<AskUserQuestionInput>("AskUserQuestion");
    // Task 全族
    assert_goal_required::<TaskCreateInput>("TaskCreate");
    assert_goal_required::<TaskUpdateInput>("TaskUpdate");
    assert_goal_required::<TaskBlockByInput>("TaskBlockBy");
    assert_goal_required::<TaskGetInput>("TaskGet");
    assert_goal_required::<TaskListInput>("TaskListGet");
    assert_goal_required::<TaskListsInput>("TaskLists");
    assert_goal_required::<TaskListCreateInput>("TaskListCreate");
    assert_goal_required::<TaskStopInput>("TaskStop");
    // BackgroundProcess 全族
    assert_goal_required::<BackgroundProcessListInput>("BackgroundProcessList");
    assert_goal_required::<BackgroundProcessStatusInput>("BackgroundProcessStatus");
    assert_goal_required::<BackgroundProcessLogsInput>("BackgroundProcessLogs");
    assert_goal_required::<BackgroundProcessStopInput>("BackgroundProcessStop");
    assert_goal_required::<BackgroundProcessWatchInput>("BackgroundProcessWatch");
    // Memory 全族
    assert_goal_required::<MemoryAddInput>("MemoryAdd");
    assert_goal_required::<MemorySearchInput>("MemorySearch");
    assert_goal_required::<MemoryListInput>("MemoryList");
    assert_goal_required::<MemoryUpdateInput>("MemoryUpdate");
    assert_goal_required::<MemoryDeleteInput>("MemoryDelete");
}

#[test]
fn goal_deserializes_to_empty_for_old_payloads() {
    // serde(default) 兼容：老 transcript 缺 goal 时反序列化为空串，
    // 由 adapter 侧以 "missing required parameter: goal" 拒绝（对齐 Bash/Agent）。
    let input: GrepInput = serde_json::from_value(serde_json::json!({"pattern": "foo"}))
        .expect("缺 goal 的老 payload 应可反序列化");
    assert!(input.goal.is_empty());
    let input: MemorySearchInput =
        serde_json::from_value(serde_json::json!({"query": "bar"})).expect("老 payload");
    assert!(input.goal.is_empty());
}
