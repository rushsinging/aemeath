use super::super::event_mapping::{sdk_event_to_tui_event, SdkEventMapping};
use super::{TuiRunEvent, TuiRuntimeEvent};

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
fn interaction_request_maps_all_body_fields_without_sdk_payload() {
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
fn runtime_event_source_does_not_reference_sdk_or_sender() {
    let source = include_str!("tui_runtime_event.rs");
    for forbidden in ["sdk::", "oneshot::Sender", "mpsc::Sender", "AgentClient"] {
        assert!(
            !source.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
}
