use sdk::{ReflectionOutputView, SdkError};

use super::accessors::AgentClientImpl;

type Result<T> = std::result::Result<T, SdkError>;

pub(super) async fn run_reflection_impl(
    me: &AgentClientImpl,
    messages: Vec<sdk::ChatMessage>,
) -> Result<ReflectionOutputView> {
    validate_reflection_config(&me.inner.context.memory_config)?;

    let runtime_messages = messages
        .into_iter()
        .map(super::mapping::message_from_sdk)
        .collect::<Vec<_>>();
    let client = me.inner.current_client.read().unwrap().clone();
    let result = crate::business::reflection::run_complete_reflection(
        crate::business::reflection::ReflectionRunMode::Forced,
        &me.inner.context.memory_config,
        &runtime_messages,
        &me.inner.cwd,
        client.as_ref(),
        &me.inner.context.system_prompt_text,
    )
    .await
    .map_err(|e| {
        let msg = match &e {
            crate::business::reflection::ReflectionError::StoreInit(detail) => {
                format!("反思记忆存储初始化失败：{detail}")
            }
            crate::business::reflection::ReflectionError::LlmCall(detail) => {
                format!("反思 LLM 调用失败：{detail}")
            }
            crate::business::reflection::ReflectionError::EmptyResponse => {
                "LLM 未返回任何反思内容".to_string()
            }
            crate::business::reflection::ReflectionError::Unparseable(detail) => {
                format!("LLM 返回的内容无法解析为反思 JSON：{detail}")
            }
            other => format!("Reflection 运行失败：{other}"),
        };
        SdkError::Internal(msg)
    })?
    .ok_or_else(|| {
        SdkError::Internal("Reflection 未执行：条件不满足（已禁用或未命中触发间隔）。".to_string())
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

pub(super) async fn apply_reflection_impl(
    me: &AgentClientImpl,
    output: ReflectionOutputView,
) -> Result<String> {
    apply_reflection_with_base_dir(
        &me.inner.context.memory_config,
        &me.inner.cwd,
        storage::api::memory_base_dir(),
        output,
    )
}

fn apply_reflection_with_base_dir(
    config: &share::config::MemoryConfig,
    cwd: &std::path::Path,
    base_dir: std::path::PathBuf,
    output: ReflectionOutputView,
) -> Result<String> {
    validate_memory_enabled_for_apply(config)?;

    if output.auto_applied {
        return Ok("Reflection 已自动应用，无需重复应用。".to_string());
    }
    if output.suggested_memories.is_empty() && output.outdated_memories.is_empty() {
        return Ok("没有可应用的 Reflection 建议。".to_string());
    }

    let reflection_output = reflection_output_from_sdk(output)?;
    let mut store = storage::api::MemoryStore::new(
        base_dir,
        storage::api::project_file_name_from_path(cwd),
        config.max_entries,
        config.similarity_threshold,
    )
    .map_err(|e| SdkError::Internal(format!("打开 MemoryStore 失败：{e}")))?;

    let applied =
        crate::business::reflection::ReflectionEngine::apply_output(&reflection_output, &mut store)
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
) -> Result<crate::business::reflection::ReflectionOutput> {
    Ok(crate::business::reflection::ReflectionOutput {
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
) -> Result<crate::business::reflection::MemorySuggestion> {
    Ok(crate::business::reflection::MemorySuggestion {
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
    use async_trait::async_trait;
    use provider::api::{
        LlmProvider, StopReason, StreamHandler, StreamResponse, SystemBlock, Usage,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;
    use tools::api::{AgentRunRequest, AgentRunner};

    struct NoopAgentRunner;

    #[async_trait]
    impl AgentRunner for NoopAgentRunner {
        async fn run_agent(&self, _request: AgentRunRequest<'_>) -> String {
            String::new()
        }

        async fn complete(
            &self,
            _prompt: &str,
            _system: &str,
            _ctx: &tools::api::ToolExecutionContext,
        ) -> String {
            String::new()
        }
    }

    struct StaticReflectionProvider {
        response: String,
        usage: Usage,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl LlmProvider for StaticReflectionProvider {
        async fn stream_message(
            &self,
            _system: &[SystemBlock],
            _messages: &[share::message::Message],
            _tool_schemas: &[serde_json::Value],
            handler: &mut dyn StreamHandler,
            _cancel: &CancellationToken,
        ) -> std::result::Result<StreamResponse, provider::LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            handler.on_text(&self.response);
            Ok(StreamResponse {
                assistant_message: share::message::Message::placeholder(
                    share::message::Role::Assistant,
                ),
                usage: self.usage.clone(),
                stop_reason: StopReason::EndTurn,
            })
        }

        fn model_name(&self) -> &str {
            "test-reflection-model"
        }

        fn provider_name(&self) -> &str {
            "test-reflection-provider"
        }

        fn set_reasoning(&self, _enabled: bool) {}

        fn is_reasoning(&self) -> bool {
            false
        }
    }

    fn build_test_client(
        memory_config: share::config::MemoryConfig,
        response: &str,
        calls: Arc<AtomicUsize>,
    ) -> AgentClientImpl {
        let cwd =
            std::env::temp_dir().join(format!("aemeath-sdk-reflection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        let client = Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            StaticReflectionProvider {
                response: response.to_string(),
                usage: Usage {
                    input_tokens: 11,
                    output_tokens: 22,
                    cached_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                calls,
            },
        )));
        let context = crate::core::port::ChatRuntimeContext {
            client: client.clone(),
            registry: Arc::new(tools::api::ToolRegistry::new()),
            system_blocks: Vec::new(),
            system_prompt_text: "真实 system prompt".to_string(),
            user_context: String::new(),
            agent_runner: Arc::new(NoopAgentRunner),
            task_store: Arc::new(storage::api::TaskStore::new()),
            skills_map: std::collections::HashMap::new(),
            hook_runner: hook::api::HookRunner::empty(cwd.display().to_string()),
            memory_config,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            allow_all: false,
            context_size: 200_000,
            verbose: false,
            resume: None,
            language: "en".to_string(),
        };
        let (change_tx, change_rx) = tokio::sync::watch::channel(sdk::ChangeSet::empty());
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
            max_tool_concurrency: 1,
            max_agent_concurrency: 1,
            _mcp_manager: Arc::new(tools::api::McpConnectionManager::with_servers(
                std::collections::HashMap::new(),
            )),
            current_client: std::sync::RwLock::new(client),
            current_cancel: Arc::new(std::sync::Mutex::new(None)),
            current_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
            frozen_chats: Arc::new(std::sync::Mutex::new(Vec::new())),
            active_summary: Arc::new(std::sync::Mutex::new(None)),
            workspace: project::api::WorkspaceService::new(std::env::temp_dir()),
            change_tx,
            change_rx,
            hook_runner: None,
            task_store: None,
            session_reminders: Arc::new(std::sync::RwLock::new(
                share::memory::SessionReminders::new(),
            )),
        };
        AgentClientImpl {
            inner: Arc::new(handle),
        }
    }

    #[tokio::test]
    async fn test_run_reflection_impl_forced_uses_llm_runner_and_returns_real_output() {
        let calls = Arc::new(AtomicUsize::new(0));
        let client = build_test_client(
            share::config::MemoryConfig::default(),
            "```json\n{\"deviations\":[\"偏离了既定计划\"],\"suggested_memories\":[{\"content\":\"用户偏好先写测试再实现\",\"category\":\"preference\"}],\"outdated_memories\":[\"old-memory-id\"],\"user_alert\":\"请关注测试覆盖\"}\n```",
            calls.clone(),
        );

        let view = run_reflection_impl(
            &client,
            vec![sdk::ChatMessage {
                role: "user".to_string(),
                content: vec![sdk::ContentBlock::text("请反思当前会话")],
                metadata: None,
            }],
        )
        .await
        .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(view.input_tokens, 11);
        assert_eq!(view.output_tokens, 22);
        assert_eq!(view.suggested_memories.len(), 1);
        assert_eq!(view.suggested_memories[0].content, "用户偏好先写测试再实现");
        assert_eq!(view.suggested_memories[0].layer, "project");
        assert_eq!(view.suggested_memories[0].category, "preference");
        assert_eq!(view.outdated_memories, vec!["old-memory-id"]);
        assert!(!view.auto_applied);
        assert!(view.content.contains("偏离了既定计划"));
        assert!(view.content.contains("用户偏好先写测试再实现"));
        assert!(view.content.contains("请关注测试覆盖"));
    }

    #[tokio::test]
    async fn test_run_reflection_impl_forced_ignores_interval_gate() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut config = share::config::MemoryConfig::default();
        config.reflection.interval_turns = 100;
        let client = build_test_client(
            config,
            "{\"deviations\":[],\"suggested_memories\":[],\"outdated_memories\":[],\"user_alert\":null}",
            calls.clone(),
        );

        run_reflection_impl(
            &client,
            vec![sdk::ChatMessage {
                role: "user".to_string(),
                content: vec![sdk::ContentBlock::text("只有一轮也要强制反思")],
                metadata: None,
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
                "{\"deviations\":[],\"suggested_memories\":[],\"outdated_memories\":[],\"user_alert\":null}",
                calls.clone(),
            );

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

    fn temp_memory_base_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "aemeath-sdk-apply-reflection-{}",
            uuid::Uuid::new_v4()
        ))
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

    #[test]
    fn test_apply_reflection_impl_writes_suggestion() {
        let config = share::config::MemoryConfig::default();
        let cwd =
            std::env::temp_dir().join(format!("aemeath-sdk-apply-cwd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        let base_dir = temp_memory_base_dir();
        let output = reflection_view(
            vec![suggestion_view(
                "project",
                "decision",
                "显式 apply 写入记忆",
            )],
            Vec::new(),
            false,
        );

        let message =
            apply_reflection_with_base_dir(&config, &cwd, base_dir.clone(), output).unwrap();
        let store = storage::api::MemoryStore::new(
            &base_dir,
            storage::api::project_file_name_from_path(&cwd),
            config.max_entries,
            config.similarity_threshold,
        )
        .unwrap();
        let memories = store
            .list(Some(share::memory::MemoryLayer::Project))
            .unwrap();

        assert!(message.contains("已应用 Reflection"));
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "显式 apply 写入记忆");
        assert_eq!(
            memories[0].category,
            share::memory::MemoryCategory::Decision
        );
        assert_eq!(memories[0].source, share::memory::MemorySource::Llm);
        assert_eq!(memories[0].layer, share::memory::MemoryLayer::Project);
        assert_eq!(memories[0].tags, vec!["reflection"]);
        let _ = std::fs::remove_dir_all(base_dir);
        let _ = std::fs::remove_dir_all(cwd);
    }

    #[test]
    fn test_apply_reflection_impl_marks_outdated() {
        let config = share::config::MemoryConfig::default();
        let cwd =
            std::env::temp_dir().join(format!("aemeath-sdk-apply-cwd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        let base_dir = temp_memory_base_dir();
        let mut store = storage::api::MemoryStore::new(
            &base_dir,
            storage::api::project_file_name_from_path(&cwd),
            config.max_entries,
            config.similarity_threshold,
        )
        .unwrap();
        let existing = share::memory::MemoryEntry::new(
            "outdated-id",
            100,
            share::memory::MemoryLayer::Project,
            share::memory::MemoryCategory::Fact,
            "旧记忆",
            share::memory::MemorySource::User,
        );
        store.add(existing).unwrap();
        let output = reflection_view(Vec::new(), vec!["outdated-id".to_string()], false);

        let message =
            apply_reflection_with_base_dir(&config, &cwd, base_dir.clone(), output).unwrap();
        let memories = store
            .list(Some(share::memory::MemoryLayer::Project))
            .unwrap();
        let outdated = memories
            .iter()
            .find(|entry| entry.id == "outdated-id")
            .unwrap();

        assert!(message.contains("标记 1 条过时记忆"));
        assert!(outdated.outdated);
        let _ = std::fs::remove_dir_all(base_dir);
        let _ = std::fs::remove_dir_all(cwd);
    }

    #[test]
    fn test_apply_reflection_impl_disabled_error() {
        let config = share::config::MemoryConfig {
            enabled: false,
            ..Default::default()
        };
        let cwd = std::env::temp_dir();
        let base_dir = temp_memory_base_dir();
        let output = reflection_view(
            vec![suggestion_view("project", "decision", "不会写入")],
            Vec::new(),
            false,
        );

        let err =
            apply_reflection_with_base_dir(&config, &cwd, base_dir.clone(), output).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("memory.enabled=false"));
        assert!(!base_dir.exists());
    }

    #[test]
    fn test_apply_reflection_impl_empty_noop() {
        let config = share::config::MemoryConfig::default();
        let cwd = std::env::temp_dir();
        let base_dir = temp_memory_base_dir();
        let output = reflection_view(Vec::new(), Vec::new(), false);

        let message =
            apply_reflection_with_base_dir(&config, &cwd, base_dir.clone(), output).unwrap();

        assert!(message.contains("没有可应用"));
        assert!(!base_dir.exists());
    }

    #[test]
    fn test_apply_reflection_impl_auto_applied_noop() {
        let config = share::config::MemoryConfig::default();
        let cwd = std::env::temp_dir();
        let base_dir = temp_memory_base_dir();
        let output = reflection_view(
            vec![suggestion_view("project", "decision", "已自动应用")],
            vec!["memory-id".to_string()],
            true,
        );

        let message =
            apply_reflection_with_base_dir(&config, &cwd, base_dir.clone(), output).unwrap();

        assert!(message.contains("已自动应用"));
        assert!(!base_dir.exists());
    }
}
