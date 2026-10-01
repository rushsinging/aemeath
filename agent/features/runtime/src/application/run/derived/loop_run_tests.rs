use crate::application::loop_engine::event_strategy::terminal_from_domain_event;
use crate::domain::agent_run::{RunId, RuntimeLifecycleEvent};

#[test]
fn terminal_domain_events_project_to_all_agent_terminal_variants() {
    let run_id = RunId::new_v7();
    let parent_run_id = Some(RunId::new_v7());
    let cases = [
        (
            RuntimeLifecycleEvent::Completed {
                run_id: run_id.clone(),
                parent_run_id: parent_run_id.clone(),
                result: "done".to_string(),
                user_cancelled_step: false,
            },
            Some(tools::published::agent::AgentRunTerminal::Completed {
                result: "done".to_string(),
            }),
        ),
        (
            RuntimeLifecycleEvent::Failed {
                run_id: run_id.clone(),
                parent_run_id: parent_run_id.clone(),
                error: "boom".to_string(),
            },
            Some(tools::published::agent::AgentRunTerminal::Failed {
                error: "boom".to_string(),
            }),
        ),
        (
            RuntimeLifecycleEvent::Terminated {
                run_id,
                parent_run_id,
                reason: sdk::RunTerminationReason::ParentStepCancelled,
            },
            Some(tools::published::agent::AgentRunTerminal::Cancelled),
        ),
    ];
    for (event, expected) in cases {
        assert_eq!(terminal_from_domain_event(&event), expected);
    }
}

#[test]
fn nonterminal_domain_event_does_not_create_agent_terminal() {
    let event = RuntimeLifecycleEvent::Started {
        run_id: RunId::new_v7(),
        parent_run_id: Some(RunId::new_v7()),
    };
    assert_eq!(terminal_from_domain_event(&event), None);
}
