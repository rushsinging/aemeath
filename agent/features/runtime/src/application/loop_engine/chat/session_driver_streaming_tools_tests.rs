/// 检查消息 content 中是否含指定文本的 ToolResult block（materialize 输出为 ToolResult）。
fn message_contains_tool_result_text(message: &Message, needle: &str) -> bool {
    message.content.iter().any(|block| match block {
        share::message::ContentBlock::ToolResult { content, text, .. } => {
            text.as_deref().is_some_and(|t| t.contains(needle))
                || content.to_string().contains(needle)
        }
        _ => false,
    })
}

struct StepBlockingTool {
    name: &'static str,
    started: Arc<tokio::sync::Notify>,
    cleaned: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl ::tools::TypedTool for StepBlockingTool {
    type Output = serde_json::Value;

    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Step-scoped streaming cancellation probe"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    fn cancellation(&self) -> ::tools::CancellationDeclaration {
        ::tools::CancellationDeclaration::Cooperative
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        context: &::tools::ToolExecutionContext,
    ) -> ::tools::TypedToolResult<Self::Output> {
        self.started.notify_one();
        context.cancellation().cancelled().await;
        self.cleaned.notify_one();
        ::tools::TypedToolResult::error(format!("{} cancelled by current Step", self.name))
    }
}

struct StepCancelledStreamingToolProvider {
    tool_name: &'static str,
    invocation_count: Arc<Mutex<usize>>,
}

#[async_trait]
impl LlmProvider for StepCancelledStreamingToolProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let invocation_number = {
            let mut count = self.invocation_count.lock().unwrap();
            *count += 1;
            *count
        };
        if invocation_number > 1 {
            return Ok(Box::pin(futures::stream::iter(vec![
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::Text("after cancellation".to_string())],
                    stop_reason: ProviderStopReason::EndTurn,
                    usage: None,
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ])));
        }
        let provider_id = ProviderToolCallId(format!("toolu_{}_cancel", self.tool_name));
        let tool_call = ProviderToolCall {
            id: provider_id.clone(),
            name: self.tool_name.to_string(),
            arguments: serde_json::json!({}),
        };
        let cancel = cancel.clone();
        let completed_call = tool_call.clone();
        let stream = futures::stream::iter(vec![
            InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
                index: 0,
                provider_id: Some(provider_id),
                name: self.tool_name.to_string(),
            }),
            InvocationEvent::Delta(InvocationDelta::ToolCallCompleted {
                index: 0,
                call: tool_call,
            }),
        ])
        .chain(futures::stream::once(async move {
            cancel.cancelled().await;
            InvocationEvent::Completed(ProviderCompletion {
                output: vec![ProviderContentBlock::ToolCall(completed_call)],
                stop_reason: ProviderStopReason::ToolUse,
                usage: None,
                effective_reasoning: ReasoningLevel::Off,
            })
        }));
        Ok(Box::pin(stream))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

async fn assert_streaming_named_tool_observes_step_cancel(tool_name: &'static str) {
    let started = Arc::new(tokio::sync::Notify::new());
    let cleaned = Arc::new(tokio::sync::Notify::new());
    let provider = Arc::new(StepCancelledStreamingToolProvider {
        tool_name,
        invocation_count: Arc::new(Mutex::new(0)),
    });
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let factory = ::tools::composition::TestCatalogExecutionFactory::new();
    factory.register(StepBlockingTool {
        name: tool_name,
        started: started.clone(),
        cleaned: cleaned.clone(),
    });
    let tool_context = crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        Default::default(),
    );
    let wired = factory.build(tool_context);
    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "cancel streaming tool",
            Vec::new(),
        ))
        .unwrap();
    let shell = test_shell_with_catalog(test_hook_port(), wired);
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider),
    );
    let active_run = shell.active_run.clone();
    let context = test_session_driver_input(sink.clone(), input_events, shell);

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        started.notified().await;
        assert_eq!(
            active_run
                .cancel_current_main(sdk::ControlDeadline::from_unix_millis(1_725_000_000_123,)),
            sdk::CancelCurrentRunOutcome::Accepted
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), cleaned.notified())
            .await
            .expect("Step cancellation must reach the running tool cleanup");
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if driver_sink
                    .events()
                    .iter()
                    .any(|event| event == "ToolResult")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled streaming tool must publish its terminal result before teardown");
        drop(input_tx);
    });

    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_session_command_driver(context),
    )
    .await
    .expect("streaming tool Step cancellation must converge");
    driver.await.expect("cancellation driver joins");
    let events = sink.events();
    assert!(
        events.iter().any(|event| event == "ToolResult"),
        "cancelled streaming {tool_name} must publish a terminal ToolResult: {events:?}"
    );
    assert!(events.iter().any(|event| event == "Cancelled"));
    assert!(!events.iter().any(|event| event == "DoneWithDuration"));
}

#[tokio::test]
async fn streaming_bash_execution_is_cancelled_by_the_current_step() {
    assert_streaming_named_tool_observes_step_cancel("Bash").await;
}

#[tokio::test]
async fn streaming_agent_execution_is_cancelled_by_the_current_step() {
    assert_streaming_named_tool_observes_step_cancel("Agent").await;
}

/// #1494：流中 ToolCallCompleted（参数完整）→ 旁路立即执行工具，不等流完整返回。
#[tokio::test]
async fn streaming_tool_call_executes_before_stream_completes() {
    let after_tool_completed = Arc::new(tokio::sync::Notify::new());
    let provider = Arc::new(StreamingToolDeltaProvider::new(
        after_tool_completed.clone(),
    ));
    let recorded = provider.recorded_messages.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    let factory = ::tools::composition::TestCatalogExecutionFactory::new();
    factory.register(NoopMarkerTool);
    let tool_ctx = crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        Default::default(),
    );
    let wired = factory.build(tool_ctx);
    let _catalog = wired.catalog();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "run streaming tool",
            Vec::new(),
        ))
        .unwrap();

    let shell = test_shell_with_catalog(test_hook_port(), wired);
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-streaming-tool-execution");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let driver_sink = sink.clone();
    let driver_after_tool = after_tool_completed.clone();
    let driver = tokio::spawn(async move {
        driver_after_tool.notified().await;
        // 等工具执行完成事件（ToolResult）出现——此时流还在 800ms 延迟中。
        loop {
            if driver_sink.events().iter().any(|e| e == "ToolResult") {
                break;
            }
            tokio::task::yield_now().await;
        }
        loop {
            if driver_sink.events().iter().any(|e| e == "DoneWithDuration") {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver completes after streaming tool + continuation");
    driver.await.expect("driver joins cleanly");

    let events = sink.events();
    let tool_result_index = events
        .iter()
        .position(|e| e == "ToolResult")
        .expect("streaming tool must execute and emit ToolResult");
    let done_index = events
        .iter()
        .position(|e| e == "DoneWithDuration")
        .expect("run must terminate");
    assert!(
        tool_result_index < done_index,
        "streaming tool execution must complete BEFORE the stream ends (execution is not deferred): tool_result={tool_result_index} done={done_index}"
    );

    // 流结束后汇总：首帧 TurnStarted 快照在工具执行前发出（不含结果）；
    // 工具结果经旁路汇总 append 后，由 continuation 的 TurnStarted 同步（最后一条快照含）。
    let synced = sink.synced_messages();
    assert!(
        synced.last().is_some_and(|snapshot| snapshot
            .iter()
            .any(|m| message_contains_tool_result_text(m, "noop-marker-result"))),
        "tool result must be materialized into message history (visible in the final sync), got {} snapshots",
        synced.len()
    );
    let recorded = recorded.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "expected two invocations (tool call + continuation)"
    );
    assert!(
        recorded[1]
            .iter()
            .any(|m| message_contains_tool_result_text(m, "noop-marker-result")),
        "continuation request must carry the tool result"
    );
}

/// 流中 ToolCallCompleted 后流失败（retry）→ 旁路缓冲丢弃，重试请求不带已执行结果。
struct StreamingToolRetryProvider {
    call_count: Arc<Mutex<usize>>,
    recorded_messages: Arc<Mutex<Vec<Vec<Message>>>>,
}

impl StreamingToolRetryProvider {
    fn new() -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
            recorded_messages: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl LlmProvider for StreamingToolRetryProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let call_num = {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            *count
        };
        self.recorded_messages
            .lock()
            .unwrap()
            .push(messages.to_vec());
        if call_num == 1 {
            // 先发完整的 ToolCallCompleted delta（旁路执行已触发），随后流失败。
            let tool_call = ProviderToolCall {
                id: ProviderToolCallId("toolu_retry_001".to_string()),
                name: "NoopMarker".to_string(),
                arguments: serde_json::json!({"marker": "retry-drop"}),
            };
            let stream = futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_retry_001".to_string())),
                    name: "NoopMarker".to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolArgumentsDelta {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_retry_001".to_string())),
                    partial_json: r#"{"marker":"retry-drop"}"#.to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolCallCompleted {
                    index: 0,
                    call: tool_call,
                }),
                InvocationEvent::Failed(ProviderError::retryable(
                    ProviderErrorKind::Protocol,
                    "stream broke after tool call",
                )),
            ]);
            Ok(Box::pin(stream))
        } else {
            // 重试：纯文本成功。
            Ok(Box::pin(futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::Text("retry succeeded".to_string())),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::Text("retry succeeded".to_string())],
                    stop_reason: ProviderStopReason::EndTurn,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(10),
                        output_tokens: Some(3),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ])))
        }
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

/// #1494：流失败 retry 时旁路结果丢弃——重试请求**不含**已执行工具结果。
#[tokio::test(start_paused = true)]
async fn streaming_tool_results_dropped_on_retry() {
    let provider = Arc::new(StreamingToolRetryProvider::new());
    let recorded = provider.recorded_messages.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    let factory = ::tools::composition::TestCatalogExecutionFactory::new();
    factory.register(NoopMarkerTool);
    let tool_ctx = crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        Default::default(),
    );
    let wired = factory.build(tool_ctx);
    let _catalog = wired.catalog();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "retry drops tool",
            Vec::new(),
        ))
        .unwrap();

    let shell = test_shell_with_catalog(test_hook_port(), wired);
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-streaming-tool-retry-drop");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let run = tokio::spawn(run_session_command_driver(ctx));
    advance_until_retry_condition(
        "retry succeeded",
        std::time::Duration::from_secs(15),
        || *provider.call_count.lock().unwrap() >= 2,
    )
    .await;
    wait_for_retry_test_condition("Main turn completed", || {
        sink.events()
            .iter()
            .any(|event| event == "DoneWithDuration")
    })
    .await;
    drop(input_tx);
    run.await.unwrap();

    let recorded = recorded.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2, "expected retry (2 invocations)");
    assert!(
        recorded[1]
            .iter()
            .all(|m| !m.text_content().contains("retry-drop")),
        "retry request must NOT carry the dropped streaming tool result, got: {:?}",
        recorded[1]
            .iter()
            .map(Message::text_content)
            .collect::<Vec<_>>()
    );
}

/// 收集一条消息中全部 ToolResult block 的 tool_use_id。
fn message_tool_result_ids(message: &Message) -> Vec<&str> {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            share::message::ContentBlock::ToolResult { tool_use_id, .. } => {
                Some(tool_use_id.as_str())
            }
            _ => None,
        })
        .collect()
}

/// #1581 根因二：同一次 invoke 内 provider retry——流 1 旁路执行的工具结果
/// 残留缓冲，流 2 成功后一并 materialize，产出无 tool_use 配对的孤儿 tool_result。
struct StreamingToolRetryOrphanProvider {
    call_count: Arc<Mutex<usize>>,
    recorded_messages: Arc<Mutex<Vec<Vec<Message>>>>,
}

impl StreamingToolRetryOrphanProvider {
    fn new() -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
            recorded_messages: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl LlmProvider for StreamingToolRetryOrphanProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let call_num = {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            *count
        };
        self.recorded_messages
            .lock()
            .unwrap()
            .push(messages.to_vec());
        let usage = || {
            Some(RawUsageSnapshot {
                input_tokens: Some(10),
                output_tokens: Some(3),
                ..RawUsageSnapshot::default()
            })
        };
        if call_num == 1 {
            // 流 1：完整 ToolCallCompleted（旁路执行已触发），随后流失败触发 retry。
            let stream = futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_retry_orphan_a".to_string())),
                    name: "NoopMarker".to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolCallCompleted {
                    index: 0,
                    call: ProviderToolCall {
                        id: ProviderToolCallId("toolu_retry_orphan_a".to_string()),
                        name: "NoopMarker".to_string(),
                        arguments: serde_json::json!({"marker": "orphan-a"}),
                    },
                }),
                InvocationEvent::Failed(ProviderError::retryable(
                    ProviderErrorKind::Protocol,
                    "stream broke after tool call",
                )),
            ]);
            Ok(Box::pin(stream))
        } else if call_num == 2 {
            // 流 2（retry）：重新发出同参数工具调用并正常完成。
            let completed_call = ProviderToolCall {
                id: ProviderToolCallId("toolu_retry_pair_b".to_string()),
                name: "NoopMarker".to_string(),
                arguments: serde_json::json!({"marker": "pair-b"}),
            };
            let stream = futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_retry_pair_b".to_string())),
                    name: "NoopMarker".to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolCallCompleted {
                    index: 0,
                    call: completed_call.clone(),
                }),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::ToolCall(completed_call)],
                    stop_reason: ProviderStopReason::ToolUse,
                    usage: usage(),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ]);
            Ok(Box::pin(stream))
        } else {
            // 流 3（工具轮次后的 continuation）：纯文本收尾。
            Ok(Box::pin(futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::Text("turn complete".to_string())),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::Text("turn complete".to_string())],
                    stop_reason: ProviderStopReason::EndTurn,
                    usage: usage(),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ])))
        }
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

/// #1581：retry 后的 continuation 请求历史中，只允许存在与 assistant tool_use
/// 配对的 tool_result——流 1 失败尝试的旁路结果不得残留为孤儿。
#[tokio::test(start_paused = true)]
async fn streaming_tool_retry_discards_stale_round_from_failed_attempt() {
    let provider = Arc::new(StreamingToolRetryOrphanProvider::new());
    let recorded = provider.recorded_messages.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    let factory = ::tools::composition::TestCatalogExecutionFactory::new();
    factory.register(NoopMarkerTool);
    let tool_ctx = crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        Default::default(),
    );
    let wired = factory.build(tool_ctx);
    let _catalog = wired.catalog();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "retry orphan pairing",
            Vec::new(),
        ))
        .unwrap();

    let shell = test_shell_with_catalog(test_hook_port(), wired);
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-streaming-tool-retry-orphan");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let run = tokio::spawn(run_session_command_driver(ctx));
    advance_until_retry_condition(
        "turn complete",
        std::time::Duration::from_secs(15),
        || *provider.call_count.lock().unwrap() >= 3,
    )
    .await;
    wait_for_retry_test_condition("Main turn completed", || {
        sink.events()
            .iter()
            .any(|event| event == "DoneWithDuration")
    })
    .await;
    drop(input_tx);
    run.await.unwrap();

    let recorded = recorded.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        3,
        "expected failed attempt + retry + continuation (3 invocations)"
    );
    let continuation_result_ids = recorded[2]
        .iter()
        .flat_map(message_tool_result_ids)
        .collect::<Vec<_>>();
    assert!(
        continuation_result_ids.contains(&"toolu_retry_pair_b"),
        "continuation request must carry the paired retry tool result, got: {continuation_result_ids:?}"
    );
    assert!(
        !continuation_result_ids.contains(&"toolu_retry_orphan_a"),
        "continuation request must NOT carry the orphaned tool result from the failed attempt, got: {continuation_result_ids:?}"
    );
}
