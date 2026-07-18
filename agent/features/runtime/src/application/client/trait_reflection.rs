use sdk::{ReflectionOutputView, SdkError};

use super::accessors::AgentClientImpl;

type Result<T> = std::result::Result<T, SdkError>;

/// Runs a forced reflection using the **committed** `MemoryPort` bound by the
/// Main Session wiring.
///
/// The memory snapshot is captured *inside* `wiring.with_shared`, so a
/// concurrent `resume_prepared` (exclusive permit) cannot swap memory while
/// reflection reads/writes it. The runtime never opens `storage::MemoryStore`.
pub(super) async fn run_reflection_impl(
    me: &AgentClientImpl,
    messages: Vec<sdk::ChatMessage>,
) -> Result<ReflectionOutputView> {
    validate_reflection_config(&me.inner.context.resources.memory_config)?;

    let runtime_messages = messages
        .into_iter()
        .map(super::mapping::message_from_sdk)
        .collect::<Vec<_>>();
    let client = me.inner.current_client.read().unwrap().clone();
    let config = me.inner.context.resources.memory_config.clone();
    let system_prompt_text = me.inner.context.resources.system_prompt_text.clone();
    let language = me.inner.context.resources.language.clone();
    let wiring = me.inner.wiring.clone();
    let wiring_for_future = wiring.clone();

    let reflection = wiring
        .with_shared(async move {
            let memory = wiring_for_future.committed_memory();
            crate::application::reflection::run_complete_reflection(
                crate::application::reflection::ReflectionRunMode::Forced,
                &config,
                &runtime_messages,
                memory.as_ref(),
                client.as_ref(),
                &system_prompt_text,
                &language,
            )
            .await
        })
        .await
        .map_err(|_| {
            SdkError::Internal("Reflection 失败：Main Session 切换门禁已关闭。".to_string())
        })?;

    let result = reflection
        .map_err(|e| {
            let msg = match &e {
                crate::application::reflection::ReflectionError::LlmCall(detail) => {
                    format!("反思 LLM 调用失败：{detail}")
                }
                crate::application::reflection::ReflectionError::EmptyResponse => {
                    "LLM 未返回任何反思内容".to_string()
                }
                crate::application::reflection::ReflectionError::Unparseable(detail) => {
                    format!("LLM 返回的内容无法解析为反思 JSON：{detail}")
                }
                other => format!("Reflection 运行失败：{other}"),
            };
            SdkError::Internal(msg)
        })?
        .ok_or_else(|| {
            SdkError::Internal(
                "Reflection 未执行：条件不满足（已禁用或未命中触发间隔）。".to_string(),
            )
        })?;

    Ok(super::mapping::reflection_output_to_sdk_with_content(
        result.output,
        result.formatted_content,
        result.input_tokens,
        result.output_tokens,
        result.auto_applied,
    ))
}

fn validate_reflection_config(config: &share::config::MemoryConfig) -> Result<()> {
    if !config.enabled {
        return Err(SdkError::Internal(
            "无法运行 Reflection：memory.enabled=false，记忆系统已禁用。".to_string(),
        ));
    }
    if !config.reflection.enabled {
        return Err(SdkError::Internal(
            "无法运行 Reflection：reflection.enabled=false，反思系统已禁用。".to_string(),
        ));
    }
    if config.reflection.interval_turns == 0 {
        return Err(SdkError::Internal(
            "无法运行 Reflection：reflection.interval_turns=0，请设置为大于 0。".to_string(),
        ));
    }
    Ok(())
}

/// Applies a reflection output to the **committed** `MemoryPort`.
///
/// Suggestions are written via `port.write` and outdated IDs via
/// `port.mark_outdated`, all under `wiring.with_shared` so a resume cannot race
/// the apply. The runtime never opens `storage::MemoryStore`.
pub(super) async fn apply_reflection_impl(
    me: &AgentClientImpl,
    output: ReflectionOutputView,
) -> Result<String> {
    validate_memory_enabled_for_apply(&me.inner.context.resources.memory_config)?;

    if output.auto_applied {
        return Ok("Reflection 已自动应用，无需重复应用。".to_string());
    }
    if output.suggested_memories.is_empty() && output.outdated_memories.is_empty() {
        return Ok("没有可应用的 Reflection 建议。".to_string());
    }

    let reflection_output = reflection_output_from_sdk(output)?;
    let wiring = me.inner.wiring.clone();
    let wiring_for_future = wiring.clone();

    let applied = wiring
        .with_shared(async move {
            let memory = wiring_for_future.committed_memory();
            crate::application::reflection::ReflectionEngine::apply_output(
                &reflection_output,
                memory.as_ref(),
            )
            .await
        })
        .await
        .map_err(|_| {
            SdkError::Internal("应用 Reflection 失败：Main Session 切换门禁已关闭。".to_string())
        })?
        .map_err(|e| SdkError::Internal(format!("应用 Reflection 失败：{e}")))?;

    Ok(format!(
        "已应用 Reflection：新增/合并 {} 条记忆，标记 {} 条过时记忆。",
        applied.suggestions_added, applied.outdated_marked
    ))
}

fn validate_memory_enabled_for_apply(config: &share::config::MemoryConfig) -> Result<()> {
    if !config.enabled {
        return Err(SdkError::Internal(
            "无法应用 Reflection：memory.enabled=false，记忆系统已禁用。".to_string(),
        ));
    }
    Ok(())
}

fn reflection_output_from_sdk(
    output: ReflectionOutputView,
) -> Result<crate::application::reflection::ReflectionOutput> {
    Ok(crate::application::reflection::ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: output
            .suggested_memories
            .into_iter()
            .map(memory_suggestion_from_sdk)
            .collect::<Result<Vec<_>>>()?,
        outdated_memories: output.outdated_memories,
        user_alert: None,
    })
}

fn memory_suggestion_from_sdk(
    memory: sdk::ReflectionMemorySuggestionView,
) -> Result<crate::application::reflection::MemorySuggestion> {
    Ok(crate::application::reflection::MemorySuggestion {
        layer: parse_memory_layer(&memory.layer)?,
        category: parse_memory_category(&memory.category)?,
        content: memory.content,
        tags: memory.tags,
        reason: String::new(),
    })
}

fn parse_memory_layer(value: &str) -> Result<share::memory::MemoryLayer> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        SdkError::Internal(format!(
            "无法应用 Reflection：未知 memory layer `{value}`。"
        ))
    })
}

fn parse_memory_category(value: &str) -> Result<share::memory::MemoryCategory> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        SdkError::Internal(format!(
            "无法应用 Reflection：未知 memory category `{value}`。"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::text_completion_stream;
    use async_trait::async_trait;
    use memory::{MemoryEntry, MemoryId, MemoryLayer};
    use provider::{InvocationStream, LlmProvider, ProviderError, SystemBlock};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio_util::sync::CancellationToken;
    use tools::{AgentRunRequest, AgentRunner};

    fn test_memory_source() -> Arc<dyn tools::MemoryPortSource> {
        struct TestSource;
        impl tools::MemoryPortSource for TestSource {
            fn current(&self) -> Arc<dyn memory::MemoryPort> {
                Arc::new(
                    memory::InMemoryMemory::new(memory::MemoryPolicy::default())
                        .expect("valid default policy"),
                )
            }
        }
        Arc::new(TestSource)
    }

    struct NoopAgentRunner;

    #[async_trait]
    impl AgentRunner for NoopAgentRunner {
        async fn run_agent(&self, _request: AgentRunRequest<'_>) -> tools::AgentRunTerminal {
            tools::AgentRunTerminal::Completed {
                result: String::new(),
            }
        }

        async fn complete(
            &self,
            _prompt: &str,
            _system: &str,
            _ctx: &tools::ToolExecutionContext,
        ) -> String {
            String::new()
        }
    }

    struct StaticReflectionProvider {
        response: String,
        input_tokens: u32,
        output_tokens: u32,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl LlmProvider for StaticReflectionProvider {
        async fn invocation_stream(
            &self,
            _scope: &provider::InvocationScope,
            _system: &[SystemBlock],
            _messages: &[share::message::Message],
            _tool_schemas: &[serde_json::Value],
            _cancel: &CancellationToken,
        ) -> std::result::Result<InvocationStream, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(text_completion_stream(
                self.response.clone(),
                self.input_tokens,
                self.output_tokens,
            ))
        }

        fn model_name(&self) -> &str {
            "test-reflection-model"
        }

        fn provider_name(&self) -> &str {
            "test-reflection-provider"
        }
    }

    /// Provider that captures the user-prompt text of the reflection call so
    /// tests can assert the memory summary was read from the bound port.
    struct CapturingReflectionProvider {
        response: String,
        captured: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl LlmProvider for CapturingReflectionProvider {
        async fn invocation_stream(
            &self,
            _scope: &provider::InvocationScope,
            _system: &[SystemBlock],
            messages: &[share::message::Message],
            _tool_schemas: &[serde_json::Value],
            _cancel: &CancellationToken,
        ) -> std::result::Result<InvocationStream, ProviderError> {
            let prompt = messages
                .iter()
                .filter_map(|message| {
                    message
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            share::message::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .next()
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.captured.lock().unwrap().push(prompt);
            Ok(text_completion_stream(self.response.clone(), 11, 22))
        }

        fn model_name(&self) -> &str {
            "test-capturing-reflection"
        }

        fn provider_name(&self) -> &str {
            "test-capturing-reflection"
        }
    }

    /// Provider whose reflection LLM call blocks until `release` is signaled,
    /// so tests can assert the caller holds the shared gate during the call.
    struct HangingReflectionProvider {
        release: Arc<tokio::sync::Notify>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl LlmProvider for HangingReflectionProvider {
        async fn invocation_stream(
            &self,
            _scope: &provider::InvocationScope,
            _system: &[SystemBlock],
            _messages: &[share::message::Message],
            _tool_schemas: &[serde_json::Value],
            _cancel: &CancellationToken,
        ) -> std::result::Result<InvocationStream, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.release.notified().await;
            Ok(text_completion_stream(
                r#"{"deviations":[],"suggested_memories":[]}"#.to_string(),
                0,
                0,
            ))
        }

        fn model_name(&self) -> &str {
            "test-hanging-reflection"
        }

        fn provider_name(&self) -> &str {
            "test-hanging-reflection"
        }
    }

    async fn build_test_client(
        memory_config: share::config::MemoryConfig,
        client: Arc<provider::LlmClient>,
    ) -> AgentClientImpl {
        let cwd =
            std::env::temp_dir().join(format!("aemeath-sdk-reflection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        let context = crate::ports::legacy::ChatRuntimeContext {
            resources: crate::application::resources::RuntimeResources {
                client: client.clone(),
                registry: Arc::new(tools::ToolRegistry::new()),
                system_blocks: Vec::new(),
                system_prompt_text: "真实 system prompt".to_string(),
                user_context: String::new(),
                agent_runner: Arc::new(NoopAgentRunner),
                tool_result_materializer:
                    crate::application::testing::test_tool_result_materializer(),
                task_store: Arc::new(storage::TaskStore::new()),
                task_access: Arc::new(task::TaskStore::new()),
                skills_map: std::collections::HashMap::new(),
                hook_runner: hook::api::HookRunner::empty(),
                memory_config,
                memory_source: test_memory_source(),
                agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
                allow_all: false,
                context_size: 200_000,
                language: "en".to_string(),
            },
            verbose: false,
            resume: None,
        };
        let config = Arc::new(config::ConfigAppService::new(Some(&cwd)));
        let workspace_views = project::wire_production_workspace(std::env::temp_dir())
            .expect("workspace 初始化成功")
            .into_views();
        let wiring = context::test_support::wire_in_memory(
            &workspace_views,
            task::wire_task().persist(),
            config.clone(),
            config.clone(),
        )
        .await;
        let config_query = wiring.config_query();
        let config_writer = wiring.config_writer();
        let handle = super::super::accessors::RuntimeHandle {
            context,
            cwd,
            resolved_model: share::config::models::ResolvedModel {
                source_key: "test".to_string(),
                source_config: share::config::models::ProviderModelsConfig::default(),
                model: share::config::models::ModelEntryConfig::default(),
                driver: "openai".to_string(),
            },
            session_id: "test-session".to_string(),
            session_tasks: context::compose_session_task_capture(task::wire_task().persist()),
            max_tool_concurrency: 1,
            max_agent_concurrency: 1,
            _mcp_manager: Arc::new(tools::McpConnectionManager::with_servers(
                std::collections::HashMap::new(),
            )),
            current_client: std::sync::RwLock::new(client),
            active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
            current_chain: Arc::new(std::sync::Mutex::new(context::session::ChatChain::default())),
            frozen_chats: Arc::new(std::sync::Mutex::new(Vec::new())),
            active_summary: Arc::new(std::sync::Mutex::new(None)),
            workspace: workspace_views,
            wiring,
            config_query,
            config_writer,
            event_sink_factory: Arc::new(|_| panic!("测试不应构造 SDK event sink")),
            session_reminders: Arc::new(std::sync::RwLock::new(
                share::memory::SessionReminders::new(),
            )),
        };
        AgentClientImpl {
            inner: Arc::new(handle),
        }
    }

    fn static_client(response: &str, calls: Arc<AtomicUsize>) -> Arc<provider::LlmClient> {
        Arc::new(provider::LlmClient::from_provider(Arc::new(
            StaticReflectionProvider {
                response: response.to_string(),
                input_tokens: 11,
                output_tokens: 22,
                calls,
            },
        )))
    }

    /// Forced reflection reads the pre-existing Project memory straight from the
    /// committed `MemoryPort`. The captured prompt must contain the seeded entry.
    #[tokio::test]
    async fn test_run_reflection_impl_forced_reads_existing_port_entries() {
        let calls = Arc::new(AtomicUsize::new(0));
        let captured = Arc::new(Mutex::new(Vec::new()));
        let response = r#"{"deviations":[],"suggested_memories":[]}"#;
        let client_arc = Arc::new(provider::LlmClient::from_provider(Arc::new(
            CapturingReflectionProvider {
                response: response.to_string(),
                captured: captured.clone(),
            },
        )));
        // Wrap so calls counter increments too (reuse the counting via a second
        // provider is unnecessary; the capturing provider is invoked once).
        let _ = calls;
        let client =
            build_test_client(share::config::MemoryConfig::default(), client_arc.clone()).await;

        // Seed the committed port, not legacy storage.
        let memory = client.inner.wiring.committed_memory();
        memory
            .write(
                MemoryEntry::new(
                    MemoryId::now_v7(),
                    1,
                    MemoryLayer::Project,
                    memory::MemoryCategory::Fact,
                    "已有项目记忆-从 port 读取",
                    memory::MemorySource::User,
                )
                .unwrap(),
            )
            .await
            .unwrap();

        run_reflection_impl(
            &client,
            vec![sdk::ChatMessage {
                role: "user".to_string(),
                content: vec![sdk::ContentBlock::text("请反思当前会话")],
                metadata: None,
                input_id: None,
            }],
        )
        .await
        .unwrap();

        let prompts = captured.lock().unwrap();
        assert_eq!(
            prompts.len(),
            1,
            "reflection should call the LLM exactly once"
        );
        assert!(
            prompts[0].contains("已有项目记忆-从 port 读取"),
            "reflection prompt must read the seeded memory from the port: {}",
            prompts[0]
        );
    }

    #[tokio::test]
    async fn test_run_reflection_impl_forced_ignores_interval_gate() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut config = share::config::MemoryConfig::default();
        config.reflection.interval_turns = 100;
        let client = build_test_client(
            config,
            static_client(
                "{\"deviations\":[],\"suggested_memories\":[],\"outdated_memories\":[],\"user_alert\":null}",
                calls.clone(),
            ),
        )
        .await;

        run_reflection_impl(
            &client,
            vec![sdk::ChatMessage {
                role: "user".to_string(),
                content: vec![sdk::ContentBlock::text("只有一轮也要强制反思")],
                metadata: None,
                input_id: None,
            }],
        )
        .await
        .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_run_reflection_impl_returns_chinese_config_errors() {
        let cases = [
            (
                share::config::MemoryConfig {
                    enabled: false,
                    ..Default::default()
                },
                "memory.enabled=false",
            ),
            (
                {
                    let mut config = share::config::MemoryConfig::default();
                    config.reflection.enabled = false;
                    config
                },
                "reflection.enabled=false",
            ),
            (
                {
                    let mut config = share::config::MemoryConfig::default();
                    config.reflection.interval_turns = 0;
                    config
                },
                "reflection.interval_turns=0",
            ),
        ];

        for (config, expected) in cases {
            let calls = Arc::new(AtomicUsize::new(0));
            let client = build_test_client(
                config,
                static_client(
                    "{\"deviations\":[],\"suggested_memories\":[],\"outdated_memories\":[],\"user_alert\":null}",
                    calls.clone(),
                ),
            )
            .await;

            let err = run_reflection_impl(&client, Vec::new()).await.unwrap_err();
            let message = err.to_string();
            assert!(message.contains(expected), "unexpected error: {message}");
            assert!(
                message.contains("无法运行 Reflection"),
                "unexpected error: {message}"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn test_apply_reflection_impl_writes_suggestion_back_to_port() {
        let config = share::config::MemoryConfig::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let client = build_test_client(config, static_client("{}", calls)).await;
        let memory = client.inner.wiring.committed_memory();

        let output = reflection_view(
            vec![suggestion_view(
                "project",
                "decision",
                "显式 apply 写回 MemoryPort",
            )],
            Vec::new(),
            false,
        );

        let message = apply_reflection_impl(&client, output).await.unwrap();
        let entries = memory.list(Some(MemoryLayer::Project));

        assert!(message.contains("已应用 Reflection"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "显式 apply 写回 MemoryPort");
        assert_eq!(entries[0].category, memory::MemoryCategory::Decision);
        assert_eq!(entries[0].source, memory::MemorySource::Llm);
        assert_eq!(entries[0].layer, MemoryLayer::Project);
        assert_eq!(entries[0].tags, vec!["reflection"]);
    }

    #[tokio::test]
    async fn test_apply_reflection_impl_marks_outdated_via_port() {
        let config = share::config::MemoryConfig::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let client = build_test_client(config, static_client("{}", calls)).await;
        let memory = client.inner.wiring.committed_memory();
        let existing = MemoryEntry::new(
            MemoryId::now_v7(),
            100,
            MemoryLayer::Project,
            memory::MemoryCategory::Fact,
            "旧记忆",
            memory::MemorySource::User,
        )
        .unwrap();
        let existing_id = existing.id;
        memory.write(existing).await.unwrap();

        let output = reflection_view(Vec::new(), vec![existing_id.to_string()], false);
        let message = apply_reflection_impl(&client, output).await.unwrap();
        let entries = memory.list(Some(MemoryLayer::Project));
        let outdated = entries
            .iter()
            .find(|entry| entry.id == existing_id)
            .unwrap();

        assert!(message.contains("标记 1 条过时记忆"));
        assert!(outdated.outdated);
    }

    #[tokio::test]
    async fn test_apply_reflection_impl_skips_invalid_and_missing_outdated_ids() {
        let config = share::config::MemoryConfig::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let client = build_test_client(config, static_client("{}", calls)).await;

        // "not-a-uuid" is unparseable; the valid UUID does not exist.
        let missing_uuid = MemoryId::now_v7();
        let output = reflection_view(
            Vec::new(),
            vec!["not-a-uuid".to_string(), missing_uuid.to_string()],
            false,
        );

        let message = apply_reflection_impl(&client, output).await.unwrap();

        // 0 marked, but the apply still succeeds (graceful handling).
        assert!(message.contains("标记 0 条过时记忆"));
    }

    #[tokio::test]
    async fn test_apply_reflection_impl_disabled_error() {
        let config = share::config::MemoryConfig {
            enabled: false,
            ..Default::default()
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let client = build_test_client(config, static_client("{}", calls)).await;
        let output = reflection_view(
            vec![suggestion_view("project", "decision", "不会写入")],
            Vec::new(),
            false,
        );

        let err = apply_reflection_impl(&client, output).await.unwrap_err();
        let message = err.to_string();

        assert!(message.contains("memory.enabled=false"));
    }

    #[tokio::test]
    async fn test_apply_reflection_impl_empty_noop() {
        let config = share::config::MemoryConfig::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let client = build_test_client(config, static_client("{}", calls)).await;
        let output = reflection_view(Vec::new(), Vec::new(), false);

        let message = apply_reflection_impl(&client, output).await.unwrap();

        assert!(message.contains("没有可应用"));
    }

    #[tokio::test]
    async fn test_apply_reflection_impl_auto_applied_noop() {
        let config = share::config::MemoryConfig::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let client = build_test_client(config, static_client("{}", calls)).await;
        let output = reflection_view(
            vec![suggestion_view("project", "decision", "已自动应用")],
            vec!["memory-id".to_string()],
            true,
        );

        let message = apply_reflection_impl(&client, output).await.unwrap();

        assert!(message.contains("已自动应用"));
    }

    /// While a forced reflection holds the shared session-switch permit (during
    /// its LLM call), an exclusive acquisition — i.e. resume — must block. This
    /// proves the caller routes memory access through `wiring.with_shared`.
    #[tokio::test]
    async fn test_forced_reflection_blocks_resume_via_shared_gate() {
        let release = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let client_arc = Arc::new(provider::LlmClient::from_provider(Arc::new(
            HangingReflectionProvider {
                release: release.clone(),
                calls: calls.clone(),
            },
        )));
        let client_impl =
            build_test_client(share::config::MemoryConfig::default(), client_arc).await;
        let gate = client_impl.inner.wiring.gate();

        // Spawn the forced reflection; it acquires the shared permit then hangs
        // on the LLM call until `release` is notified.
        let task = {
            let client_impl = client_impl.clone();
            tokio::spawn(async move {
                run_reflection_impl(
                    &client_impl,
                    vec![sdk::ChatMessage {
                        role: "user".to_string(),
                        content: vec![sdk::ContentBlock::text("reflect")],
                        metadata: None,
                        input_id: None,
                    }],
                )
                .await
            })
        };
        // Give the spawned task a chance to acquire the shared permit and enter
        // the hanging LLM call.
        for _ in 0..50 {
            if calls.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            calls.load(Ordering::SeqCst) > 0,
            "reflection LLM never started"
        );

        // While the shared permit is held, an exclusive acquire (resume) blocks.
        let exclusive = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            gate.acquire_owned_exclusive(),
        )
        .await;
        assert!(
            exclusive.is_err(),
            "exclusive (resume) must be blocked while forced reflection holds the shared gate"
        );

        // Releasing the reflection drops the shared permit; resume can proceed.
        release.notify_waiters();
        let _ = task.await.unwrap();
        let exclusive = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            gate.acquire_owned_exclusive(),
        )
        .await;
        assert!(
            exclusive.is_ok(),
            "exclusive (resume) must succeed after forced reflection releases the shared gate"
        );
    }

    fn reflection_view(
        suggestions: Vec<sdk::ReflectionMemorySuggestionView>,
        outdated_memories: Vec<String>,
        auto_applied: bool,
    ) -> sdk::ReflectionOutputView {
        sdk::ReflectionOutputView {
            content: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            suggested_memories: suggestions,
            outdated_memories,
            auto_applied,
        }
    }

    fn suggestion_view(
        layer: &str,
        category: &str,
        content: &str,
    ) -> sdk::ReflectionMemorySuggestionView {
        sdk::ReflectionMemorySuggestionView {
            content: content.to_string(),
            layer: layer.to_string(),
            category: category.to_string(),
            tags: vec!["reflection".to_string()],
        }
    }
}
