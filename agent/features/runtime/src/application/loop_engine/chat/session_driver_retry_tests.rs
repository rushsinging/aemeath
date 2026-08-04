#[tokio::test(start_paused = true)]
async fn main_partial_stream_failure_retries_without_rollback() {
    let provider = Arc::new(ScriptedInvocationProvider::new(vec![
        vec![
            InvocationEvent::Delta(InvocationDelta::Text("partial".to_string())),
            retryable_stream_failure(),
        ],
        vec![
            InvocationEvent::Delta(InvocationDelta::Text("complete".to_string())),
            successful_completion("complete"),
        ],
    ]));
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    input_tx
        .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
        .unwrap();

    let ctx = retry_main_context(provider.clone(), sink.clone(), input_events);
    let run = tokio::spawn(run_session_command_driver(ctx));
    advance_until_retry_condition(
        "successful retry",
        std::time::Duration::from_secs(11),
        || provider.calls() == 2,
    )
    .await;
    wait_for_retry_test_condition("completed Main turn", || {
        sink.events()
            .iter()
            .any(|event| event == "DoneWithDuration")
    })
    .await;
    drop(input_tx);
    run.await.unwrap();

    assert_eq!(provider.calls(), 2);
    let events = sink.events();
    let partial = events
        .iter()
        .position(|event| event == "Text:partial")
        .unwrap();
    let retry = events
        .iter()
        .position(|event| event == "ModelInvocationRetrying:2:10000")
        .unwrap();
    let complete = events
        .iter()
        .position(|event| event == "Text:complete")
        .unwrap();
    assert!(partial < retry && retry < complete, "events: {events:?}");
    assert!(
        !events.iter().any(|event| event.starts_with("ApiError:")),
        "events: {events:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn main_empty_completion_retries_and_succeeds() {
    let provider = Arc::new(ScriptedInvocationProvider::new(vec![
        vec![empty_completion()],
        vec![
            InvocationEvent::Delta(InvocationDelta::Text("complete".to_string())),
            successful_completion("complete"),
        ],
    ]));
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    input_tx
        .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
        .unwrap();

    let (ctx, wiring) =
        retry_main_context_with_wiring(provider.clone(), sink.clone(), input_events);
    let run = tokio::spawn(run_session_command_driver(ctx));
    advance_until_retry_condition(
        "successful empty-completion retry",
        std::time::Duration::from_secs(11),
        || provider.calls() == 2,
    )
    .await;
    wait_for_retry_test_condition("completed Main turn", || {
        sink.events()
            .iter()
            .any(|event| event == "DoneWithDuration")
    })
    .await;
    drop(input_tx);
    run.await.unwrap();

    assert_eq!(provider.calls(), 2);
    let events = sink.events();
    assert!(
        events
            .iter()
            .any(|event| event == "ModelInvocationRetrying:2:10000"),
        "events: {events:?}"
    );
    assert!(events.iter().any(|event| event == "Text:complete"));
    assert!(!events.iter().any(|event| event == "Text:"));
    assert!(!events.iter().any(|event| event.starts_with("ApiError:")));
    assert!(sink.synced_messages().iter().flatten().all(|message| {
        message.role != Role::Assistant || !message.text_content().trim().is_empty()
    }));
    let committed = wiring.committed_session();
    let committed_messages = committed
        .run_slices
        .iter()
        .flat_map(|slice| slice.steps.iter())
        .filter_map(|step| step.outcome.as_ref())
        .flat_map(|outcome| outcome.messages.iter());
    let committed_assistant_texts = committed_messages
        .filter(|message| message.role == Role::Assistant)
        .map(Message::text_content)
        .collect::<Vec<_>>();
    assert_eq!(
        committed_assistant_texts,
        vec!["complete"],
        "only the valid terminal assistant response may be finalized"
    );
}

#[tokio::test(start_paused = true)]
async fn main_empty_completion_exhaustion_fails_instead_of_completing() {
    let provider = Arc::new(ScriptedInvocationProvider::new(
        (0..11).map(|_| vec![empty_completion()]).collect(),
    ));
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    input_tx
        .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
        .unwrap();

    let ctx = retry_main_context(provider.clone(), sink.clone(), input_events);
    let run = tokio::spawn(run_session_command_driver(ctx));
    for expected_calls in 2..=11 {
        advance_until_retry_condition(
            "next empty completion retry",
            std::time::Duration::from_secs(121),
            || provider.calls() == expected_calls,
        )
        .await;
    }
    wait_for_retry_test_condition("exhaustion ApiError", || {
        sink.events()
            .iter()
            .any(|event| event.starts_with("ApiError:"))
    })
    .await;
    drop(input_tx);
    run.await.unwrap();

    assert_eq!(provider.calls(), 11);
    let events = sink.events();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.starts_with("ModelInvocationRetrying:"))
            .count(),
        10,
        "events: {events:?}"
    );
    let retry_events = events
        .iter()
        .filter(|event| event.starts_with("ModelInvocationRetrying:"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        retry_events,
        vec![
            "ModelInvocationRetrying:2:10000",
            "ModelInvocationRetrying:3:20146",
            "ModelInvocationRetrying:4:40219",
            "ModelInvocationRetrying:5:80041",
            "ModelInvocationRetrying:6:120000",
            "ModelInvocationRetrying:7:120000",
            "ModelInvocationRetrying:8:120000",
            "ModelInvocationRetrying:9:120000",
            "ModelInvocationRetrying:10:120000",
            "ModelInvocationRetrying:11:120000",
        ],
        "retry attempts and capped delays must remain observable: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .find(|event| event.starts_with("ApiError:"))
            .map(String::as_str),
        Some("ApiError:loop adapter error: protocol error: provider completed without assistant text or tool call"),
        "the final ApiError must preserve the last empty-terminal failure: {events:?}"
    );
}

fn test_hook_port() -> Arc<dyn HookPort> {
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: "true".to_string(),
            timeout: 5,
        }],
    );
    Arc::new(hook::build_dispatcher(&HooksConfig { events }).unwrap())
}

fn blocking_then_success_hook_port(flag_path: &std::path::Path) -> Arc<dyn HookPort> {
    let flag_path_str = flag_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import pathlib, sys, json; \
                 p=pathlib.Path(\"{flag_path}\"); \
                 sys.exit(0 if p.exists() else (p.parent.mkdir(parents=True, exist_ok=True), \
                 p.write_text(\"blocked\"), print(\"{{\\\"continue\\\":false,\\\"stopReason\\\":\\\"fix before stopping\\\"}}\"), 2)[3])'",
                flag_path = flag_path_str,
            ),
            timeout: 5,
        }],
    );
    Arc::new(hook::build_dispatcher(&HooksConfig { events }).unwrap())
}

fn delayed_blocking_then_success_hook_port(flag_path: &std::path::Path) -> Arc<dyn HookPort> {
    let flag_path_str = flag_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import pathlib, sys, time, json; \
                 p=pathlib.Path(\"{flag_path}\"); \
                 sys.exit(0 if p.exists() else (p.parent.mkdir(parents=True, exist_ok=True), \
                 p.write_text(\"blocked\"), time.sleep(0.2), print(\"{{\\\"continue\\\":false,\\\"stopReason\\\":\\\"fix before stopping\\\"}}\"), 2)[4])'",
                flag_path = flag_path_str,
            ),
            timeout: 5,
        }],
    );
    Arc::new(hook::build_dispatcher(&HooksConfig { events }).unwrap())
}

