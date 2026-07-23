use super::{sdk_event_to_tui_event, SdkEventMapping};
use crate::tui::adapter::tui_runtime_event::{TuiRunEvent, TuiRuntimeEvent};

#[test]
fn run_cancelling_keeps_identity_instead_of_becoming_empty_message() {
    let run_id = sdk::RunId::new("run-1");

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::RunCancelling {
        run_id: run_id.clone(),
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::Run {
            run_id: actual,
            parent_run_id: None,
            event: TuiRunEvent::Cancelling,
        }) if actual.as_str() == run_id.as_str()
    ));
}

#[test]
fn interaction_request_keeps_request_run_and_body_identity() {
    let request_id = sdk::InteractionRequestId::new("request-1");
    let run_id = sdk::RunId::new("run-1");
    let expected_request_id = request_id.as_str().to_string();
    let expected_run_id = run_id.as_str().to_string();
    let request = sdk::InteractionRequest {
        id: request_id,
        run_id,
        body: sdk::InteractionRequestBody::ToolApproval(sdk::ToolApprovalPrompt {
            tool_name: "Bash".to_string(),
            args_summary: "rm -rf target".to_string(),
            risk_level: sdk::RiskLevel::High,
        }),
    };

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::InteractionRequested { request });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::InteractionRequested(request))
            if request.request_id.as_str() == expected_request_id
                && request.run_id.as_str() == expected_run_id
    ));
}

#[test]
fn ask_user_sender_is_explicitly_split_out_of_runtime_event() {
    let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::AskUserBatch {
        items: Vec::new(),
        reply_tx,
    });

    assert!(matches!(mapped, SdkEventMapping::LegacyAskUser { items, .. } if items.is_empty()));
}
