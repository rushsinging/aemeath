use super::*;
use provider::ProviderError;
use runtime::{ProviderBindingData, ProviderBuildSpecData, ProviderFactory};
use share::config::Config;
use std::sync::atomic::{AtomicUsize, Ordering};

fn document_with_value(value: serde_json::Value) -> config::GlobalConfigDocument {
    config::GlobalConfigDocument {
        revision: config::GlobalConfigRevision::from_digest("test-revision"),
        value,
    }
}

#[test]
fn connect_global_user_agent_reads_trimmed_document_value() {
    let document =
        document_with_value(serde_json::json!({"api": {"user_agent": "  company-agent/7.7  "}}));
    assert_eq!(
        connect_global_user_agent_from_document(&document),
        Some("company-agent/7.7".to_string())
    );
}

#[test]
fn connect_global_user_agent_treats_blank_value_as_unconfigured() {
    let document = document_with_value(serde_json::json!({"api": {"user_agent": "   "}}));
    assert_eq!(connect_global_user_agent_from_document(&document), None);
}

#[test]
fn connect_global_user_agent_keeps_config_default_when_api_section_is_absent() {
    // `api.user_agent` 是带默认值的 legacy 字段；缺失时必须与
    // `ConfigSnapshot::user_agent()` 一致地回落到默认配置 UA。
    let document = document_with_value(serde_json::json!({"models": {"providers": {}}}));
    assert_eq!(
        connect_global_user_agent_from_document(&document),
        Some(Config::default().api.user_agent)
    );
}

#[test]
fn connect_global_user_agent_degrades_for_non_object_document() {
    let document = document_with_value(serde_json::json!("not-an-object"));
    assert_eq!(
        connect_global_user_agent_from_document(&document),
        Some(Config::default().api.user_agent)
    );
}

/// 快照提取（canonical 格式）：`reasoning_effort` 与模型级 `apiStyle`
/// 必须进入 `ExistingProviderSnapshot`，供编辑已有 Provider 时回填。
#[test]
fn provider_snapshot_reads_canonical_reasoning_effort_and_model_level_api_style() {
    let document = document_with_value(serde_json::json!({
        "models": {"providers": {"Zhipu": {
            "baseUrl": "https://zhipu.test",
            "driver": "zhipu",
            "models": [{
                "id": "glm-5.3",
                "contextWindow": 256_000,
                "max_tokens": 16_000,
                "reasoning_effort": "high",
                "apiStyle": "responses"
            }]
        }}}
    }));
    let snapshots = existing_provider_snapshots(&document.value);
    let snapshot = snapshots.first().expect("snapshot extracted");
    assert_eq!(snapshot.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(snapshot.api_style.as_deref(), Some("responses"));
    assert_eq!(
        snapshot.models.first().unwrap().reasoning_effort.as_deref(),
        Some("high")
    );
}

/// 快照提取（存量漂移格式）：camelCase `reasoningEffort` 与 provider 级
/// `apiStyle` 必须兼容读取，避免存量配置在编辑时丢失推理档位与接口风格。
#[test]
fn provider_snapshot_reads_legacy_camel_case_and_provider_level_api_style() {
    let document = document_with_value(serde_json::json!({
        "models": {"providers": {"Zhipu": {
            "baseUrl": "https://zhipu.test",
            "driver": "zhipu",
            "apiStyle": "responses",
            "models": [{
                "id": "glm-5.3",
                "contextWindow": 256_000,
                "max_tokens": 16_000,
                "reasoningEffort": "xhigh"
            }]
        }}}
    }));
    let snapshots = existing_provider_snapshots(&document.value);
    let snapshot = snapshots.first().expect("snapshot extracted");
    assert_eq!(snapshot.reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(snapshot.api_style.as_deref(), Some("responses"));
    assert_eq!(
        snapshot.models.first().unwrap().reasoning_effort.as_deref(),
        Some("xhigh")
    );
}

/// 快照提取（canonical 优先）：两种格式并存时以 snake_case 为准。
#[test]
fn provider_snapshot_prefers_canonical_reasoning_effort_over_legacy_alias() {
    let document = document_with_value(serde_json::json!({
        "models": {"providers": {"Zhipu": {
            "baseUrl": "https://zhipu.test",
            "driver": "zhipu",
            "models": [{
                "id": "glm-5.3",
                "contextWindow": 256_000,
                "max_tokens": 16_000,
                "reasoning_effort": "low",
                "reasoningEffort": "high"
            }]
        }}}
    }));
    let snapshots = existing_provider_snapshots(&document.value);
    assert_eq!(
        snapshots.first().unwrap().reasoning_effort.as_deref(),
        Some("low")
    );
}

#[test]
fn logging_init_decision_initializes_when_no_logger_exists() {
    assert_eq!(
        logging_init_decision(None, LoggingOutputMode::File).unwrap(),
        LoggingInitDecision::Initialize
    );
}

#[test]
fn logging_init_decision_is_idempotent_for_same_output_mode() {
    assert_eq!(
        logging_init_decision(Some(LoggingOutputMode::Stderr), LoggingOutputMode::Stderr).unwrap(),
        LoggingInitDecision::AlreadyInitialized
    );
}

#[test]
fn logging_init_decision_rejects_conflicting_output_mode() {
    let error = logging_init_decision(Some(LoggingOutputMode::File), LoggingOutputMode::Stderr)
        .unwrap_err();
    assert!(error.contains("already initialized"));
    assert!(error.contains("File"));
    assert!(error.contains("Stderr"));
}

#[derive(Default)]
struct CountingProviderFactory {
    build_calls: AtomicUsize,
}

struct ReportedUsageProvider {
    invocation_count: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl runtime::ProviderPort for ReportedUsageProvider {
    fn capabilities(
        &self,
        model: &provider::ModelIdData,
    ) -> Result<provider::ModelCapabilityData, ProviderError> {
        Ok(provider::ModelCapabilityData {
            model: model.clone(),
            supports_tools: true,
            supports_parallel_tool_calls: true,
            supports_streaming: true,
            reasoning: provider::ReasoningCapabilityData::new(
                [share::reasoning::ReasoningLevel::Off],
                provider::ReasoningMappingKindData::None,
            )?,
            context_limit: Some(128_000),
            output_limit: Some(8_192),
        })
    }

    async fn invoke(
        &self,
        _request: provider::InvocationRequestData,
        _cancellation: &dyn runtime::CancellationSignal,
    ) -> Result<provider::InvocationStreamData, ProviderError> {
        let invocation_index = self.invocation_count.fetch_add(1, Ordering::SeqCst);
        let completion = match invocation_index {
            0 => provider::ProviderCompletionData {
                output: vec![provider::ProviderContentBlockData::ToolCall(
                    provider::ProviderToolCallData {
                        id: provider::ProviderToolCallIdData("call-sub-agent".to_string()),
                        name: "Agent".to_string(),
                        arguments: serde_json::json!({
                            "description": "record child usage",
                            "prompt": "finish successfully",
                            "agent": "coder"
                        }),
                    },
                )],
                stop_reason: provider::ProviderStopReasonData::ToolUse,
                usage: Some(provider::RawUsageSnapshotData {
                    input_tokens: Some(13),
                    output_tokens: Some(8),
                    cache_write_tokens: Some(0),
                    cache_read_tokens: None,
                    reasoning_tokens: None,
                }),
                effective_reasoning: share::reasoning::ReasoningLevel::Off,
            },
            1 => provider::ProviderCompletionData {
                output: vec![provider::ProviderContentBlockData::Text(
                    "sub-agent complete".to_string(),
                )],
                stop_reason: provider::ProviderStopReasonData::EndTurn,
                usage: Some(provider::RawUsageSnapshotData {
                    input_tokens: Some(21),
                    output_tokens: Some(5),
                    cache_write_tokens: None,
                    cache_read_tokens: Some(3),
                    reasoning_tokens: None,
                }),
                effective_reasoning: share::reasoning::ReasoningLevel::Off,
            },
            _ => provider::ProviderCompletionData {
                output: vec![provider::ProviderContentBlockData::Text(
                    "main-agent complete".to_string(),
                )],
                stop_reason: provider::ProviderStopReasonData::EndTurn,
                usage: None,
                effective_reasoning: share::reasoning::ReasoningLevel::Off,
            },
        };
        Ok(Box::pin(futures_util::stream::iter(vec![
            provider::InvocationEventData::Completed(completion),
        ])))
    }
}

struct ReportedUsageProviderFactory {
    invocation_count: Arc<AtomicUsize>,
}

impl ReportedUsageProviderFactory {
    fn new() -> Self {
        Self {
            invocation_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ProviderFactory for ReportedUsageProviderFactory {
    fn build(&self, spec: ProviderBuildSpecData) -> Result<ProviderBindingData, ProviderError> {
        Ok(ProviderBindingData {
            provider: Arc::new(ReportedUsageProvider {
                invocation_count: self.invocation_count.clone(),
            }),
            model: spec.model,
            max_tokens: spec.max_tokens,
            requested_reasoning: spec.requested_reasoning,
            context_window: spec.context_window,
        })
    }
}

impl ProviderFactory for CountingProviderFactory {
    fn build(&self, spec: ProviderBuildSpecData) -> Result<ProviderBindingData, ProviderError> {
        self.build_calls.fetch_add(1, Ordering::SeqCst);
        crate::provider::provider_factory().build(spec)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn successful_runtime_invocation_persists_versioned_session_usage_jsonl() {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    let agents_dir = temp.path().join("agents");
    std::fs::create_dir_all(&root).expect("create project root");
    std::fs::create_dir_all(&agents_dir).expect("create agents dir");
    std::fs::write(
        agents_dir.join("aemeath.json"),
        serde_json::json!({
            "models": {
                "default": "local/test-model",
                "providers": {
                    "local": {
                        "baseUrl": "http://127.0.0.1:1/v1",
                        "apiKey": "test-api-key",
                        "driver": "openai",
                        "models": [{
                            "id": "test-model",
                            "name": "Test Model",
                            "input": ["text"],
                            "contextWindow": 128000,
                            "max_tokens": 8192
                        }]
                    }
                }
            },
            "agents": {
                "names": {
                    "coder": {
                        "role": "coder",
                        "model": "local/test-model",
                        "description": "test child usage"
                    }
                }
            }
        })
        .to_string(),
    )
    .expect("write global config");
    std::fs::write(agents_dir.join("mcp.json"), "{\"mcpServers\":{}}").expect("write mcp config");
    let args = AgentArgs {
        cwd: Some(root),
        api_key: Some("test-api-key".to_string()),
        base_url: Some("http://127.0.0.1:1/v1".to_string()),
        model: Some("local/test-model".to_string()),
        context_size: 128_000,
        ..AgentArgs::default()
    };
    let config = config::wire_project_config_with_agents_dir(
        args.cwd.as_deref().expect("cwd"),
        &agents_dir,
        wire_config_override_store(&agents_dir).expect("override store"),
        cli_config_input(&args),
    )
    .await
    .expect("config wiring");
    let workspace =
        wire_workspace_with_config(args.cwd.as_deref().expect("cwd"), &config).expect("workspace");
    let gateways = FeatureGateways::new(
        Arc::new(ReportedUsageProviderFactory::new()),
        configured_policy(&config),
    );
    let mut assembly =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config, &agents_dir)
            .await
            .expect("runtime assembly");
    let session_id = assembly.client.session_id();
    let (input_sender, input_receiver) = tokio::sync::mpsc::unbounded_channel();
    input_sender
        .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
        .expect("send user input");
    drop(input_sender);
    let input_port = TestInputEventPort::new(input_receiver);
    let mut stream = sdk::AgentClient::chat(
        &assembly.client,
        sdk::ChatRequest {
            ingress: Arc::new(input_port),
        },
    )
    .await
    .expect("chat stream");
    while stream.recv().await.is_some() {}

    assembly
        .audit
        .take()
        .expect("session audit")
        .shutdown()
        .await;
    let usage_path = agents_dir
        .join("audit/usage")
        .join(format!("{session_id}.jsonl"));
    let source = std::fs::read_to_string(&usage_path).expect("read usage jsonl");
    let lines: Vec<_> = source.lines().collect();
    assert_eq!(lines.len(), 2, "source: {source}");
    let envelopes: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("usage envelope"))
        .collect();
    assert!(envelopes
        .iter()
        .all(|envelope| envelope["schema_version"] == 1));
    assert!(envelopes
        .iter()
        .all(|envelope| envelope["record"]["session_id"] == session_id));
    assert!(envelopes
        .iter()
        .all(|envelope| envelope["record"]["provider"] == "local"));
    assert!(envelopes
        .iter()
        .all(|envelope| envelope["record"]["model"] == "test-model"));
    let run_ids: std::collections::HashSet<_> = envelopes
        .iter()
        .map(|envelope| {
            envelope["record"]["run_id"]
                .as_str()
                .expect("run id")
                .to_string()
        })
        .collect();
    assert_eq!(run_ids.len(), 2, "source: {source}");
    assert!(envelopes.iter().any(|envelope| {
        envelope["record"]["input_tokens"] == 13
            && envelope["record"]["output_tokens"] == 8
            && envelope["record"]["cache_write_tokens"] == 0
    }));
    assert!(envelopes.iter().any(|envelope| {
        envelope["record"]["input_tokens"] == 21
            && envelope["record"]["output_tokens"] == 5
            && envelope["record"]["cache_read_tokens"] == 3
    }));
    assert!(envelopes
        .iter()
        .all(|envelope| envelope["record"]["run_step_id"].as_str().is_some()));
    assert!(envelopes
        .iter()
        .all(|envelope| envelope["record"]["model_invocation_id"].as_str().is_some()));
}

struct TestInputEventPort {
    receiver: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<sdk::ChatInputEvent>>,
}

impl TestInputEventPort {
    fn new(receiver: tokio::sync::mpsc::UnboundedReceiver<sdk::ChatInputEvent>) -> Self {
        Self {
            receiver: tokio::sync::Mutex::new(receiver),
        }
    }
}

impl sdk::ChatInputEventPort for TestInputEventPort {
    fn recv_next<'a>(&'a self) -> sdk::InputEventOptFuture<'a> {
        Box::pin(async move { self.receiver.lock().await.recv().await })
    }

    fn drain_input_events<'a>(&'a self) -> sdk::InputEventFuture<'a> {
        Box::pin(async move {
            let mut receiver = self.receiver.lock().await;
            let mut events = Vec::new();
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
            events
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn build_agent_client_with_gateways_consumes_injected_provider() {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    let agents_dir = temp.path().join("agents");
    std::fs::create_dir_all(&root).expect("create project root");
    std::fs::create_dir_all(&agents_dir).expect("create agents dir");
    std::fs::write(
        agents_dir.join("aemeath.json"),
        serde_json::json!({
            "models": {
                "default": "local/test-model",
                "providers": {
                    "local": {
                        "baseUrl": "http://127.0.0.1:1/v1",
                        "apiKey": "test-api-key",
                        "driver": "openai",
                        "models": [{
                            "id": "test-model",
                            "name": "Test Model",
                            "input": ["text"],
                            "contextWindow": 8192,
                            "max_tokens": 1024
                        }]
                    }
                }
            }
        })
        .to_string(),
    )
    .expect("write config");
    std::fs::write(agents_dir.join("mcp.json"), r#"{"mcpServers":{}}"#).expect("write MCP config");

    let provider = Arc::new(CountingProviderFactory::default());
    let gateways = FeatureGateways::new(provider.clone(), policy::allow_all());
    let args = AgentArgs {
        cwd: Some(root),
        api_key: Some("test-api-key".to_string()),
        base_url: Some("http://127.0.0.1:1/v1".to_string()),
        model: Some("local/test-model".to_string()),
        context_size: 8192,
        ..Default::default()
    };

    let result = build_agent_client_with_gateways(args, gateways, &agents_dir).await;

    result.expect("build client with injected gateways");
    assert_eq!(provider.build_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn typed_bootstrap_output_mode_constructs_logging_settings() {
    let snapshot = ConfigSnapshot::new(Config::default());

    let file = logging_settings_from_bootstrap(
        &snapshot,
        Path::new("/fallback/logs"),
        sdk::LoggingOutputMode::File,
        sdk::NativeStderrMode::RouteToLogs,
    );
    let stderr = logging_settings_from_bootstrap(
        &snapshot,
        Path::new("/fallback/logs"),
        sdk::LoggingOutputMode::Stderr,
        sdk::NativeStderrMode::Preserve,
    );

    assert_eq!(file.output_mode(), LoggingOutputMode::File);
    assert_eq!(
        file.native_stderr_routing(),
        NativeStderrRouting::AppendToFile
    );
    assert_eq!(stderr.output_mode(), LoggingOutputMode::Stderr);
    assert_eq!(
        stderr.native_stderr_routing(),
        NativeStderrRouting::Preserve
    );
}

#[test]
fn snapshot_mapping_preserves_all_logging_settings() {
    let mut config = Config::default();
    config.logging.level = "aemeath:tui=debug,aemeath:agent:runtime=trace".to_string();
    config.logging.max_bytes = 42;
    config.logging.max_backups = 3;
    config.logging.retention_days = 14;
    config.logging.logs_dir = Some("custom/logs".to_string());
    let settings = logging_settings_from_snapshot(
        &ConfigSnapshot::new(config),
        Path::new("/fallback/logs"),
        LoggingOutputMode::Stderr,
        NativeStderrRouting::AppendToFile,
    );

    assert_eq!(settings.logs_dir(), PathBuf::from("custom/logs"));
    assert_eq!(settings.max_bytes(), 42);
    assert_eq!(settings.max_backups(), 3);
    assert_eq!(settings.retention_days(), 14);
    assert_eq!(settings.output_mode(), LoggingOutputMode::Stderr);
}

#[test]
fn snapshot_mapping_uses_default_logs_dir_when_config_is_absent() {
    let settings = logging_settings_from_snapshot(
        &ConfigSnapshot::new(Config::default()),
        Path::new("/fallback/logs"),
        LoggingOutputMode::File,
        NativeStderrRouting::Preserve,
    );
    assert_eq!(settings.logs_dir(), PathBuf::from("/fallback/logs"));
}
