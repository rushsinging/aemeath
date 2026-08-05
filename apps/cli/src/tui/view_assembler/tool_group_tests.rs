use super::{classify_tool_name, plan_display_units, DisplayUnitPlan, ToolGroupCandidate};
use crate::tui::view_model::output::ToolGroupKind;

#[test]
fn classifies_explore_tools_with_stable_display_kind() {
    for tool_name in ["Read", "Glob", "Grep"] {
        assert_eq!(
            classify_tool_name(tool_name),
            Some(ToolGroupKind::Explore),
            "expected {tool_name} to belong to Explore",
        );
    }
}

#[test]
fn classifies_run_and_write_tools_without_cross_category_fallback() {
    assert_eq!(classify_tool_name("Bash"), Some(ToolGroupKind::Run));
    for tool_name in ["Write", "Edit"] {
        assert_eq!(
            classify_tool_name(tool_name),
            Some(ToolGroupKind::Write),
            "expected {tool_name} to belong to Write",
        );
    }
}

#[test]
fn classifies_only_the_explicit_task_allowlist() {
    for tool_name in [
        "TaskCreate",
        "TaskUpdate",
        "TaskBlockBy",
        "TaskListGet",
        "TaskLists",
        "TaskListCreate",
        "TaskListComplete",
        "TaskGet",
        "TaskStop",
    ] {
        assert_eq!(
            classify_tool_name(tool_name),
            Some(ToolGroupKind::Tasks),
            "expected {tool_name} to belong to Tasks",
        );
    }
}

#[test]
fn leaves_unknown_and_task_prefixed_tools_unclassified() {
    for tool_name in [
        "TaskList",
        "TaskFutureTool",
        "taskCreate",
        "mcp__custom_tool",
        "UnknownTool",
    ] {
        assert_eq!(
            classify_tool_name(tool_name),
            None,
            "expected {tool_name} to remain independently displayed",
        );
    }
}

#[test]
fn exposes_the_user_facing_explore_title() {
    assert_eq!(ToolGroupKind::Explore.title(), "Explore");
    assert_ne!(ToolGroupKind::Explore.title(), "Explor");
}

#[test]
fn plans_two_or_more_consecutive_tool_calls_as_one_group() {
    let inputs = vec![
        ToolGroupCandidate::tool_call("item-1", "call-1", "Read", "step-1"),
        ToolGroupCandidate::tool_call("item-2", "call-2", "Glob", "step-1"),
        ToolGroupCandidate::tool_call("item-3", "call-3", "Grep", "step-1"),
    ];

    assert_eq!(
        plan_display_units(&inputs),
        vec![DisplayUnitPlan::ToolGroup {
            group_id: "tool-group:step-1:call-1".to_string(),
            kind: ToolGroupKind::Explore,
            member_ids: vec![
                "call-1".to_string(),
                "call-2".to_string(),
                "call-3".to_string()
            ],
            attached_result_ids: Vec::new(),
        }]
    );
}

#[test]
fn keeps_single_tool_call_as_an_independent_display_unit() {
    let inputs = vec![ToolGroupCandidate::tool_call(
        "item-1", "call-1", "Read", "step-1",
    )];

    assert_eq!(
        plan_display_units(&inputs),
        vec![DisplayUnitPlan::Single {
            item_id: "item-1".to_string(),
            attached_result_ids: Vec::new(),
        }]
    );
}

#[test]
fn cuts_groups_at_non_tool_output_and_category_changes() {
    let inputs = vec![
        ToolGroupCandidate::tool_call("item-1", "call-1", "Read", "step-1"),
        ToolGroupCandidate::tool_call("item-2", "call-2", "Glob", "step-1"),
        ToolGroupCandidate::boundary("item-3", "step-1"),
        ToolGroupCandidate::tool_call("item-4", "call-4", "Grep", "step-1"),
        ToolGroupCandidate::tool_call("item-5", "call-5", "Bash", "step-1"),
    ];

    assert_eq!(
        plan_display_units(&inputs),
        vec![
            DisplayUnitPlan::ToolGroup {
                group_id: "tool-group:step-1:call-1".to_string(),
                kind: ToolGroupKind::Explore,
                member_ids: vec!["call-1".to_string(), "call-2".to_string()],
                attached_result_ids: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-3".to_string(),
                attached_result_ids: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-4".to_string(),
                attached_result_ids: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-5".to_string(),
                attached_result_ids: Vec::new(),
            },
        ]
    );
}

#[test]
fn matching_results_are_transparent_but_orphans_cut_and_remain_visible() {
    let inputs = vec![
        ToolGroupCandidate::tool_call("item-1", "call-1", "Read", "step-1"),
        ToolGroupCandidate::result("result-1", "call-1", "step-1"),
        ToolGroupCandidate::tool_call("item-2", "call-2", "Glob", "step-1"),
        ToolGroupCandidate::result("orphan-1", "missing-call", "step-1"),
        ToolGroupCandidate::tool_call("item-3", "call-3", "Grep", "step-1"),
    ];

    assert_eq!(
        plan_display_units(&inputs),
        vec![
            DisplayUnitPlan::ToolGroup {
                group_id: "tool-group:step-1:call-1".to_string(),
                kind: ToolGroupKind::Explore,
                member_ids: vec!["call-1".to_string(), "call-2".to_string()],
                attached_result_ids: vec!["result-1".to_string()],
            },
            DisplayUnitPlan::Single {
                item_id: "orphan-1".to_string(),
                attached_result_ids: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-3".to_string(),
                attached_result_ids: Vec::new(),
            },
        ]
    );
}

#[test]
fn does_not_group_calls_across_run_step_boundaries() {
    let inputs = vec![
        ToolGroupCandidate::tool_call("item-1", "call-1", "Read", "step-1"),
        ToolGroupCandidate::tool_call("item-2", "call-2", "Glob", "step-2"),
    ];

    assert_eq!(
        plan_display_units(&inputs),
        vec![
            DisplayUnitPlan::Single {
                item_id: "item-1".to_string(),
                attached_result_ids: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-2".to_string(),
                attached_result_ids: Vec::new(),
            },
        ]
    );
}

#[test]
fn keeps_group_id_stable_when_a_matching_call_is_appended() {
    let initial_inputs = vec![
        ToolGroupCandidate::tool_call("item-1", "call-1", "Read", "step-1"),
        ToolGroupCandidate::tool_call("item-2", "call-2", "Glob", "step-1"),
    ];
    let appended_inputs = [
        initial_inputs.clone(),
        vec![ToolGroupCandidate::tool_call(
            "item-3", "call-3", "Grep", "step-1",
        )],
    ]
    .concat();

    let initial_group_id = plan_display_units(&initial_inputs)
        .into_iter()
        .find_map(|unit| match unit {
            DisplayUnitPlan::ToolGroup { group_id, .. } => Some(group_id),
            DisplayUnitPlan::Single { .. } => None,
        })
        .expect("initial calls form a group");
    let appended_group_id = plan_display_units(&appended_inputs)
        .into_iter()
        .find_map(|unit| match unit {
            DisplayUnitPlan::ToolGroup { group_id, .. } => Some(group_id),
            DisplayUnitPlan::Single { .. } => None,
        })
        .expect("appended calls remain a group");

    assert_eq!(initial_group_id, appended_group_id);
}
