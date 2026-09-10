use sdk::{
    AgentId, ApprovalDecision, InteractionCancelReason, InteractionCommandOutcome,
    InteractionReply, InteractionReplyError, InteractionRequestBody, InteractionRequestId,
    OptionItem, PlanApprovalPrompt, RiskLevel, RunStepId, StuckDiagnostic, ToolApprovalPrompt,
    UserAnswer, UserQuestion,
};

fn assert_json_round_trip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_string(value).expect("published value should serialize");
    let restored: T = serde_json::from_str(&json).expect("published value should deserialize");
    assert_eq!(&restored, value);
}

#[test]
fn published_identity_round_trips_as_uuidv7_strings() {
    let run_step_id = RunStepId::new_v7();
    let agent_id = AgentId::new_v7();
    let request_id = InteractionRequestId::new_v7();

    assert_json_round_trip(&run_step_id);
    assert_json_round_trip(&agent_id);
    assert_json_round_trip(&request_id);
    assert_eq!(run_step_id.as_uuid().get_version_num(), 7);
    assert_eq!(agent_id.as_uuid().get_version_num(), 7);
    assert_eq!(request_id.as_uuid().get_version_num(), 7);
}

#[test]
fn published_identity_rejects_non_v7_wire_values() {
    let v4_json = "\"550e8400-e29b-41d4-a716-446655440000\"";
    assert!(serde_json::from_str::<RunStepId>(v4_json).is_err());
    assert!(serde_json::from_str::<AgentId>(v4_json).is_err());
    assert!(serde_json::from_str::<InteractionRequestId>(v4_json).is_err());
}

#[test]
fn every_interaction_request_body_round_trips() {
    let bodies = [
        InteractionRequestBody::UserQuestions(vec![UserQuestion {
            prompt: "choose".to_string(),
            options: vec![
                OptionItem::new("a", "first choice"),
                OptionItem::new("b", "second choice"),
            ],
            allow_multi: true,
        }]),
        InteractionRequestBody::ToolApproval(ToolApprovalPrompt {
            tool_name: "Bash".to_string(),
            args_summary: "cargo test".to_string(),
            risk_level: RiskLevel::Medium,
        }),
        InteractionRequestBody::PlanApproval(PlanApprovalPrompt {
            plan_title: "migration".to_string(),
            steps: vec!["freeze contract".to_string()],
        }),
        InteractionRequestBody::HardPause(StuckDiagnostic {
            reason: "repeated action".to_string(),
            recent_actions: vec!["Read".to_string(), "Read".to_string()],
        }),
    ];

    for body in bodies {
        assert_json_round_trip(&body);
    }
}

#[test]
fn user_question_options_serialize_with_title_and_description() {
    let question = UserQuestion {
        prompt: "choose".to_string(),
        options: vec![OptionItem::new("a", "first choice")],
        allow_multi: false,
    };
    let value = serde_json::to_value(&question).expect("serialize question");
    assert_eq!(value["options"][0]["title"], "a");
    assert_eq!(value["options"][0]["description"], "first choice");
}

#[test]
fn user_question_options_still_deserialize_legacy_plain_strings() {
    // 历史会话落盘的 options 是纯字符串数组；resume 时必须可恢复。
    let legacy = serde_json::json!({
        "prompt": "continue?",
        "options": ["yes", "no"],
        "allow_multi": false
    });
    let question: UserQuestion = serde_json::from_value(legacy).expect("legacy options");
    assert_eq!(question.options[0].title, "yes");
    assert!(question.options[0].description.is_none());
    assert_eq!(question.options[1].title, "no");
}

#[test]
fn user_question_options_still_deserialize_object_without_description() {
    // 历史对象缺 description 也必须可恢复（title_only 展示）。
    let legacy = serde_json::json!({
        "prompt": "continue?",
        "options": [{"title": "yes"}],
        "allow_multi": false
    });
    let question: UserQuestion = serde_json::from_value(legacy).expect("legacy object options");
    assert_eq!(question.options[0].title, "yes");
    assert!(question.options[0].description.is_none());
}

#[test]
fn every_interaction_reply_and_outcome_round_trips() {
    let replies = [
        InteractionReply::UserQuestions(vec![UserAnswer("a".to_string())]),
        InteractionReply::ToolApproval(ApprovalDecision::Approve),
        InteractionReply::PlanApproval(ApprovalDecision::Deny {
            reason: Some("revise".to_string()),
        }),
        InteractionReply::HardPauseContinue,
    ];
    for reply in replies {
        assert_json_round_trip(&reply);
    }

    let outcomes = [
        InteractionCommandOutcome::Accepted,
        InteractionCommandOutcome::NotFound,
        InteractionCommandOutcome::AlreadyCompleted,
        InteractionCommandOutcome::InvalidReply(InteractionReplyError::VariantMismatch),
        InteractionCommandOutcome::RunCancelling,
    ];
    for outcome in outcomes {
        assert_json_round_trip(&outcome);
    }

    assert_json_round_trip(&InteractionCancelReason::UserCancelled);
}
