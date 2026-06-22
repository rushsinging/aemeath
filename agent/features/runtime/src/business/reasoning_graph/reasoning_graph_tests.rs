use super::*;
use provider::api::ReasoningLevel;

fn enabled_config() -> GraphRuntimeConfig {
    GraphRuntimeConfig {
        enabled: true,
        ..Default::default()
    }
}

// === ReasoningNode::default_effort ===

#[test]
fn test_node_default_effort() {
    assert_eq!(ReasoningNode::Idle.default_effort(), ReasoningLevel::Off);
    assert_eq!(
        ReasoningNode::Explore.default_effort(),
        ReasoningLevel::Medium
    );
    assert_eq!(ReasoningNode::Plan.default_effort(), ReasoningLevel::High);
    assert_eq!(ReasoningNode::Execute.default_effort(), ReasoningLevel::Low);
    assert_eq!(
        ReasoningNode::Verify.default_effort(),
        ReasoningLevel::Medium
    );
}

// === ReasoningGraph 基本查询 ===

#[test]
fn test_graph_starts_idle() {
    let graph = ReasoningGraph::new(enabled_config());
    assert_eq!(graph.current_node(), ReasoningNode::Idle);
    assert_eq!(graph.current_effort(), ReasoningLevel::Off);
}

#[test]
fn test_enabled_reflects_config() {
    let graph = ReasoningGraph::new(GraphRuntimeConfig::default());
    assert!(!graph.enabled());

    let graph = ReasoningGraph::new(enabled_config());
    assert!(graph.enabled());
}

// === UserMessage 转移 ===

#[test]
fn test_user_message_first_turn_defaults_explore() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::UserMessage {
        text: "fix the bug".to_string(),
        turn_count: 1,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}

#[test]
fn test_user_message_complex_intent_goes_plan() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::UserMessage {
        text: "请设计一个新的架构方案".to_string(),
        turn_count: 1,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Plan);
}

#[test]
fn test_user_message_english_complex_intent_goes_plan() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::UserMessage {
        text: "I need to refactor the entire module".to_string(),
        turn_count: 1,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Plan);
}

#[test]
fn test_user_message_subsequent_turn_simple_goes_explore() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::UserMessage {
        text: "继续".to_string(),
        turn_count: 3,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}

// === ToolCompleted 转移 ===

#[test]
fn test_tool_read_goes_explore() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.current = ReasoningNode::Execute;
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Read".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}

#[test]
fn test_tool_grep_goes_explore() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Grep".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}

#[test]
fn test_tool_edit_goes_execute() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Edit".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Execute);
}

#[test]
fn test_tool_write_goes_execute() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Write".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Execute);
}

#[test]
fn test_tool_error_always_goes_plan() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.current = ReasoningNode::Execute;
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Edit".to_string(),
        bash_command: None,
        is_error: true,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Plan);
}

// === Bash 分类 ===

#[test]
fn test_bash_cargo_test_goes_verify() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("cargo test".to_string()),
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Verify);
}

#[test]
fn test_bash_clippy_goes_verify() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("cargo clippy --workspace".to_string()),
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Verify);
}

#[test]
fn test_bash_git_log_goes_explore() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("git log --oneline -5".to_string()),
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}

#[test]
fn test_bash_git_diff_goes_explore() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("git diff HEAD".to_string()),
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}

#[test]
fn test_bash_default_goes_execute() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("echo hello && rm tmp.txt".to_string()),
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Execute);
}

// === TextOnly / TurnBoundary ===

#[test]
fn test_text_only_goes_idle() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.current = ReasoningNode::Explore;
    graph.transition(GraphSignal::TextOnly);
    assert_eq!(graph.current_node(), ReasoningNode::Idle);
}

#[test]
fn test_turn_boundary_preserves_node() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.current = ReasoningNode::Execute;
    let changed = graph.transition(GraphSignal::TurnBoundary);
    assert!(!changed);
    assert_eq!(graph.current_node(), ReasoningNode::Execute);
}

// === transition 返回值 ===

#[test]
fn test_transition_returns_true_on_change() {
    let mut graph = ReasoningGraph::new(enabled_config());
    let changed = graph.transition(GraphSignal::UserMessage {
        text: "hello".to_string(),
        turn_count: 1,
    });
    assert!(changed);
}

#[test]
fn test_transition_returns_false_on_no_change() {
    let mut graph = ReasoningGraph::new(enabled_config());
    graph.current = ReasoningNode::Explore;
    let changed = graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Read".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: None,
    });
    assert!(!changed); // Explore → Explore
}

// === current_effort ===

#[test]
fn test_current_effort_reflects_node() {
    let mut graph = ReasoningGraph::new(enabled_config());

    graph.current = ReasoningNode::Explore;
    assert_eq!(graph.current_effort(), ReasoningLevel::Medium);

    graph.current = ReasoningNode::Plan;
    assert_eq!(graph.current_effort(), ReasoningLevel::High);

    graph.current = ReasoningNode::Execute;
    assert_eq!(graph.current_effort(), ReasoningLevel::Low);

    graph.current = ReasoningNode::Verify;
    assert_eq!(graph.current_effort(), ReasoningLevel::Medium);

    graph.current = ReasoningNode::Idle;
    assert_eq!(graph.current_effort(), ReasoningLevel::Off);
}

// === Config 覆盖 ===

#[test]
fn test_config_override_changes_effort() {
    let config = GraphRuntimeConfig {
        enabled: true,
        max_reasoning: ReasoningLevel::Max,
        explore_effort: Some(ReasoningLevel::High),
        plan_effort: None,
        execute_effort: None,
        verify_effort: None,
    };
    let graph = ReasoningGraph::new(config);
    let _ = graph;

    let mut g = ReasoningGraph::new(enabled_config());
    g.current = ReasoningNode::Explore;
    // 默认 effort = Medium
    assert_eq!(g.current_effort(), ReasoningLevel::Medium);

    // 用覆盖配置
    let config = GraphRuntimeConfig {
        enabled: true,
        max_reasoning: ReasoningLevel::Max,
        explore_effort: Some(ReasoningLevel::High),
        plan_effort: None,
        execute_effort: None,
        verify_effort: None,
    };
    let mut g2 = ReasoningGraph::new(config);
    g2.current = ReasoningNode::Explore;
    assert_eq!(g2.current_effort(), ReasoningLevel::High);
}

// === 典型工作流序列 ===

#[test]
fn test_typical_workflow_explore_then_execute_then_verify() {
    let mut graph = ReasoningGraph::new(enabled_config());

    // 1. 用户发消息
    graph.transition(GraphSignal::UserMessage {
        text: "修复 bug".to_string(),
        turn_count: 1,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);

    // 2. LLM 读文件
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Read".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);

    // 3. LLM 编辑文件
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Edit".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Execute);

    // 4. LLM 跑测试
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("cargo test".to_string()),
        is_error: false,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Verify);

    // 5. 测试失败 → 回 Plan
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("cargo test".to_string()),
        is_error: true,
        declared_phase: None,
    });
    assert_eq!(graph.current_node(), ReasoningNode::Plan);
}

// === has_complex_intent 关键词覆盖 ===

#[test]
fn test_complex_intent_keywords() {
    assert!(has_complex_intent("请排查这个问题的根因"));
    assert!(has_complex_intent("为什么测试失败了"));
    assert!(has_complex_intent("investigate the root cause"));
    assert!(has_complex_intent("refactor this module"));
    assert!(!has_complex_intent("fix the typo"));
    assert!(!has_complex_intent("run the tests"));
}

// === Phase 2: LLM 声明 phase ===

#[test]
fn test_declared_phase_overrides_classify() {
    let mut graph = ReasoningGraph::new(enabled_config());

    // LLM 声明 verify，即使 tool 是 Bash(git diff)（classify 会判为 Explore）
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("git diff".to_string()),
        is_error: false,
        declared_phase: Some("verify".to_string()),
    });
    assert_eq!(graph.current_node(), ReasoningNode::Verify);
}

#[test]
fn test_declared_phase_explore_overrides_execute_tool() {
    let mut graph = ReasoningGraph::new(enabled_config());

    // LLM 用 Edit 但声明 explore（在编辑前先理解）
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Edit".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: Some("explore".to_string()),
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}

#[test]
fn test_declared_phase_plan_overrides_everything() {
    let mut graph = ReasoningGraph::new(enabled_config());

    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Read".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: Some("plan".to_string()),
    });
    assert_eq!(graph.current_node(), ReasoningNode::Plan);
}

#[test]
fn test_declared_phase_none_falls_back_to_classify() {
    let mut graph = ReasoningGraph::new(enabled_config());

    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("git log".to_string()),
        is_error: false,
        declared_phase: None,
    });
    // 无声明 → fallback 到 classify → git log = Explore
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}

#[test]
fn test_declared_phase_invalid_falls_back_to_classify() {
    let mut graph = ReasoningGraph::new(enabled_config());

    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Edit".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: Some("turbo".to_string()), // 无效值
    });
    // 无效声明 → fallback 到 classify → Edit = Execute
    assert_eq!(graph.current_node(), ReasoningNode::Execute);
}

#[test]
fn test_declared_phase_error_still_overrides() {
    let mut graph = ReasoningGraph::new(enabled_config());

    // tool_error 优先级最高，即使声明了 explore 也去 Plan
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Bash".to_string(),
        bash_command: Some("cargo test".to_string()),
        is_error: true,
        declared_phase: Some("explore".to_string()),
    });
    assert_eq!(graph.current_node(), ReasoningNode::Plan);
}

#[test]
fn test_declared_phase_case_insensitive() {
    let mut graph = ReasoningGraph::new(enabled_config());

    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Read".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: Some("VERIFY".to_string()),
    });
    assert_eq!(graph.current_node(), ReasoningNode::Verify);
}

#[test]
fn test_declared_phase_variant_forms() {
    let mut graph = ReasoningGraph::new(enabled_config());

    // -ing 形式也应识别
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: "Read".to_string(),
        bash_command: None,
        is_error: false,
        declared_phase: Some("exploring".to_string()),
    });
    assert_eq!(graph.current_node(), ReasoningNode::Explore);
}
