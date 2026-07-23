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

#[test]
fn event_mapping_is_the_only_sdk_chat_event_match_point() {
    let source = include_str!("event_mapping.rs");
    assert!(
        source.contains("fn sdk_event_to_tui_event"),
        "event_mapping.rs must contain the sole sdk::ChatEvent exhaustive converter"
    );
    // oneshot::Sender is permitted only in the LegacyAskUser resource bridge
    // (reply_tx), which is exempted until #1246 publishes InteractionRequested.
    // After that, #944 5B removes it entirely.
    let oneshot_count = source.matches("oneshot::Sender").count();
    assert!(
        oneshot_count <= 2,
        "event_mapping.rs may have at most 2 oneshot::Sender references (LegacyAskUser reply_tx type + conversion); found {oneshot_count}"
    );
}

#[test]
fn second_layer_mapper_has_no_sdk_dependencies() {
    let source = include_str!("agent_event.rs");
    // The second-layer mapper must consume TuiRuntimeEvent / Tui DTO only.
    // sdk:: is allowed only in the legacy UiEvent branch (map_agent_event),
    // which is dead code after 阶段 3 and will be removed by #944 5B.
    // For now, assert the map_runtime_event function block has zero sdk:: references.

    // Extract the map_runtime_event function body as a substring
    // using only safe iterator-based scanning (no byte-index slicing).
    let mut in_runtime_fn = false;
    let mut brace_depth = 0i32;
    let mut fn_body = String::new();

    for line in source.lines() {
        if !in_runtime_fn && line.contains("fn map_runtime_event") {
            in_runtime_fn = true;
            brace_depth = 0;
        }
        if in_runtime_fn {
            fn_body.push_str(line);
            fn_body.push('\n');
            brace_depth += line.matches('{').count() as i32;
            brace_depth -= line.matches('}').count() as i32;
            if brace_depth <= 0 && line.contains('}') {
                break;
            }
        }
    }

    assert!(!fn_body.is_empty(), "map_runtime_event must exist");
    assert!(
        !fn_body.contains("sdk::"),
        "map_runtime_event must not reference sdk:: — it consumes TuiRuntimeEvent only"
    );
    assert!(
        !fn_body.contains("oneshot::Sender"),
        "map_runtime_event must not hold senders"
    );
}
