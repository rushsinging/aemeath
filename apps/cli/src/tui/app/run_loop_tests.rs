use super::*;

fn context() -> crate::tui::adapter::tui_runtime_event::TuiRunContext {
    crate::tui::adapter::tui_runtime_event::TuiRunContext {
        chat_id: "batch-chat".to_string(),
        run_id: "batch-turn".to_string(),
    }
}

#[test]
fn runtime_batch_keeps_terminal_event_behind_stream_chunks() {
    let (tx, mut rx) = mpsc::channel(512);
    for index in 0..200 {
        tx.try_send(TuiRuntimeEvent::AssistantTextDelta {
            context: context(),
            delta: index.to_string(),
        })
        .unwrap();
    }
    tx.try_send(TuiRuntimeEvent::Done {
        context: context(),
        duration_ms: None,
    })
    .unwrap();

    let first = rx.try_recv().unwrap();
    let batch = collect_runtime_batch(first, &mut rx);

    assert_eq!(batch.len(), 201);
    assert!(matches!(batch.last(), Some(TuiRuntimeEvent::Done { .. })));
    assert!(rx.try_recv().is_err());
}

#[test]
fn runtime_batch_is_bounded_to_keep_terminal_input_responsive() {
    let (tx, mut rx) = mpsc::channel(512);
    for index in 0..300 {
        tx.try_send(TuiRuntimeEvent::AssistantTextDelta {
            context: context(),
            delta: index.to_string(),
        })
        .unwrap();
    }

    let first = rx.try_recv().unwrap();
    let batch = collect_runtime_batch(first, &mut rx);

    assert_eq!(batch.len(), MAX_RUNTIME_EVENTS_PER_FRAME);
    assert_eq!(rx.len(), 300 - MAX_RUNTIME_EVENTS_PER_FRAME);
}

#[test]
fn runtime_batch_stops_at_session_reset_effect_barrier() {
    let (tx, mut rx) = mpsc::channel(8);
    tx.try_send(TuiRuntimeEvent::AssistantTextDelta {
        context: context(),
        delta: "before reset".to_string(),
    })
    .unwrap();
    tx.try_send(TuiRuntimeEvent::SessionReset).unwrap();
    tx.try_send(TuiRuntimeEvent::AssistantTextDelta {
        context: context(),
        delta: "after reset".to_string(),
    })
    .unwrap();

    let first = rx.try_recv().unwrap();
    let batch = collect_runtime_batch(first, &mut rx);

    assert_eq!(batch.len(), 2);
    assert!(matches!(batch.last(), Some(TuiRuntimeEvent::SessionReset)));
    assert!(
        matches!(rx.try_recv(), Ok(TuiRuntimeEvent::AssistantTextDelta { delta, .. }) if delta == "after reset")
    );
}
