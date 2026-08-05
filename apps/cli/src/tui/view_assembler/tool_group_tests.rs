use super::{
    classify_tool_name, plan_display_units, timeline_candidate, DisplayUnitPlan, ToolGroupCandidate,
};
use crate::tui::model::conversation::ids::{ChatId, ChatRunId, ToolCallId};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::{ToolCall, ToolCallStatus};
use crate::tui::model::output_timeline::{OutputTimelineItem, TimelineToolCallRef};
use crate::tui::view_assembler::output_tool_lookup::ConversationToolLookup;
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
fn live_timeline_adapter_reads_tool_name_and_run_boundary_from_model() {
    let chat_id = ChatId::new("chat-live");
    let run_id = ChatRunId::new("run-live");
    let tool_id = ToolCallId::new("tool-1");
    let mut conversation = ConversationModel::default();
    let mut call = ToolCall::pending(
        tool_id.clone(),
        crate::tui::model::conversation::ids::ToolStreamKey::new(
            chat_id.clone(),
            run_id.clone(),
            "Read",
            0,
        ),
    );
    call.status = ToolCallStatus::Ready;
    let mut run = crate::tui::model::conversation::chat_turn::ChatRun::new(run_id.clone(), 0);
    run.tool_calls.push(call);
    conversation
        .chats
        .push(crate::tui::model::conversation::chat::Chat {
            id: chat_id.clone(),
            user_submission: String::new(),
            status: crate::tui::model::conversation::chat::ChatStatus::Running,
            runs: vec![run],
        });
    let item = OutputTimelineItem::ToolCall {
        reference: TimelineToolCallRef::new(chat_id, run_id, tool_id),
    };
    let lookup = ConversationToolLookup::new(&conversation);

    assert_eq!(
        timeline_candidate(&item, &lookup),
        ToolGroupCandidate::tool_call(
            "tool-call-chat-live/run-live/tool-1",
            "tool-1",
            "Read",
            "run-live",
        )
    );
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
            attached_results: Vec::new(),
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
            attached_results: Vec::new(),
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
                attached_results: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-3".to_string(),
                attached_results: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-4".to_string(),
                attached_results: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-5".to_string(),
                attached_results: Vec::new(),
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
                attached_results: vec![super::AttachedToolResult {
                    item_id: "result-1".to_string(),
                    call_id: "call-1".to_string()
                }],
            },
            DisplayUnitPlan::Single {
                item_id: "orphan-1".to_string(),
                attached_results: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-3".to_string(),
                attached_results: Vec::new(),
            },
        ]
    );
}

#[test]
fn records_matching_results_against_their_exact_tool_call_identity() {
    let inputs = vec![
        ToolGroupCandidate::tool_call("item-1", "call-1", "Read", "step-1"),
        ToolGroupCandidate::result("result-1", "call-1", "step-1"),
        ToolGroupCandidate::tool_call("item-2", "call-2", "Glob", "step-1"),
        ToolGroupCandidate::result("result-2", "call-2", "step-1"),
    ];

    assert_eq!(
        plan_display_units(&inputs),
        vec![DisplayUnitPlan::ToolGroup {
            group_id: "tool-group:step-1:call-1".to_string(),
            kind: ToolGroupKind::Explore,
            member_ids: vec!["call-1".to_string(), "call-2".to_string()],
            attached_results: vec![
                super::AttachedToolResult {
                    item_id: "result-1".to_string(),
                    call_id: "call-1".to_string(),
                },
                super::AttachedToolResult {
                    item_id: "result-2".to_string(),
                    call_id: "call-2".to_string(),
                },
            ],
        }]
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
                attached_results: Vec::new(),
            },
            DisplayUnitPlan::Single {
                item_id: "item-2".to_string(),
                attached_results: Vec::new(),
            },
        ]
    );
}

#[test]
fn caps_each_group_at_twenty_tool_calls() {
    let inputs = (1..=41)
        .map(|index| {
            ToolGroupCandidate::tool_call(
                &format!("item-{index}"),
                &format!("call-{index}"),
                "Edit",
                "step-1",
            )
        })
        .collect::<Vec<_>>();

    let plans = plan_display_units(&inputs);

    assert_eq!(plans.len(), 3);
    assert!(matches!(
        &plans[0],
        DisplayUnitPlan::ToolGroup {
            group_id,
            member_ids,
            ..
        } if group_id == "tool-group:step-1:call-1" && member_ids.len() == 20
    ));
    assert!(matches!(
        &plans[1],
        DisplayUnitPlan::ToolGroup {
            group_id,
            member_ids,
            ..
        } if group_id == "tool-group:step-1:call-21" && member_ids.len() == 20
    ));
    assert_eq!(
        plans[2],
        DisplayUnitPlan::Single {
            item_id: "item-41".to_string(),
            attached_results: Vec::new(),
        }
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
