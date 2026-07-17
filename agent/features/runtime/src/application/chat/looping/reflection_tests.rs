use super::*;
use async_trait::async_trait;
use provider::{LegacyStreamSink, LlmProvider};
use provider::{StopReason, StreamResponse, SystemBlock, Usage};
use share::memory::{MemoryCategory, MemoryLayer, MemorySource};
use std::sync::Arc;
use storage::MemoryStore;
use tokio_util::sync::CancellationToken;

struct StaticReflectionProvider {
    response: String,
    usage: Usage,
}

#[async_trait]
impl LlmProvider for StaticReflectionProvider {
    async fn legacy_stream_message(
        &self,
        _scope: &provider::InvocationScope,
        _system: &[SystemBlock],
        _messages: &[share::message::Message],
        _tool_schemas: &[serde_json::Value],
        handler: &mut dyn LegacyStreamSink,
        _cancel: &CancellationToken,
    ) -> Result<StreamResponse, provider::LlmError> {
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
}

fn build_client(response: &str) -> provider::LlmClient {
    build_client_with_usage(response, 0, 0)
}

fn build_client_with_usage(
    response: &str,
    input_tokens: u32,
    output_tokens: u32,
) -> provider::LlmClient {
    provider::LlmClient::from_provider(Arc::new(StaticReflectionProvider {
        response: response.to_string(),
        usage: Usage {
            input_tokens,
            output_tokens,
            cached_tokens: None,
            cache_creation_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        },
    }))
}

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("aemeath-{name}-{}", uuid::Uuid::new_v4()))
}

#[test]
fn test_should_run_turn_reflection_on_interval_completed_without_tool_calls() {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 3;

    assert!(should_run_turn_reflection(
        &config,
        6,
        false,
        &StopReason::MaxTokens,
        false,
    ));
}

#[test]
fn test_should_run_turn_reflection_on_interval_end_turn_with_tool_calls() {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 3;

    assert!(should_run_turn_reflection(
        &config,
        6,
        true,
        &StopReason::EndTurn,
        false,
    ));
}

#[test]
fn test_should_run_turn_reflection_false_when_not_interval() {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 3;

    assert!(!should_run_turn_reflection(
        &config,
        5,
        false,
        &StopReason::EndTurn,
        false,
    ));
}

#[test]
fn test_should_run_turn_reflection_false_when_memory_disabled() {
    let config = share::config::MemoryConfig {
        enabled: false,
        ..Default::default()
    };

    assert!(!should_run_turn_reflection(
        &config,
        10,
        false,
        &StopReason::EndTurn,
        false,
    ));
}

#[test]
fn test_should_run_turn_reflection_false_when_reflection_disabled() {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.enabled = false;

    assert!(!should_run_turn_reflection(
        &config,
        10,
        false,
        &StopReason::EndTurn,
        false,
    ));
}

#[test]
fn test_should_run_turn_reflection_false_when_interval_zero() {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 0;

    assert!(!should_run_turn_reflection(
        &config,
        10,
        false,
        &StopReason::EndTurn,
        false,
    ));
}

#[test]
fn test_should_run_turn_reflection_false_when_tool_calls_not_end_turn() {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 3;

    assert!(!should_run_turn_reflection(
        &config,
        6,
        true,
        &StopReason::ToolUse,
        false,
    ));
}

#[test]
fn test_should_run_turn_reflection_false_when_before_finish_gate_continues() {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 3;

    assert!(!should_run_turn_reflection(
        &config,
        6,
        false,
        &StopReason::EndTurn,
        true,
    ));
}

#[tokio::test]
async fn test_run_complete_reflection_disabled_returns_none() {
    let cwd = temp_dir("reflection-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let base_dir = temp_dir("reflection-memory");
    let client = build_client(r#"{"suggested_memories":[]}"#);
    let config = share::config::MemoryConfig {
        enabled: false,
        ..Default::default()
    };

    let result = run_complete_reflection_with_base_dir(
        ReflectionRunMode::Interval { turn_count: 10 },
        &config,
        &[share::message::Message::user("不会运行")],
        &cwd,
        &client,
        "system prompt",
        base_dir.clone(),
        "zh",
    )
    .await
    .unwrap();

    assert!(result.is_none());
    let _ = std::fs::remove_dir_all(cwd);
    let _ = std::fs::remove_dir_all(base_dir);
}

#[tokio::test]
async fn test_run_complete_reflection_interval_miss_returns_none() {
    let cwd = temp_dir("reflection-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let base_dir = temp_dir("reflection-memory");
    let client = build_client(r#"{"suggested_memories":[]}"#);
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 5;

    let result = run_complete_reflection_with_base_dir(
        ReflectionRunMode::Interval { turn_count: 4 },
        &config,
        &[share::message::Message::user("未命中 interval")],
        &cwd,
        &client,
        "system prompt",
        base_dir.clone(),
        "zh",
    )
    .await
    .unwrap();

    assert!(result.is_none());
    let _ = std::fs::remove_dir_all(cwd);
    let _ = std::fs::remove_dir_all(base_dir);
}

#[tokio::test]
async fn test_run_complete_reflection_forced_skips_interval_hit_check() {
    let cwd = temp_dir("reflection-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let base_dir = temp_dir("reflection-memory");
    let response = r#"{"deviations":["forced 已运行"],"suggested_memories":[]}"#;
    let client = build_client_with_usage(response, 12, 34);
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 5;

    let result = run_complete_reflection_with_base_dir(
        ReflectionRunMode::Forced,
        &config,
        &[share::message::Message::user("不需要命中 interval")],
        &cwd,
        &client,
        "system prompt",
        base_dir.clone(),
        "zh",
    )
    .await
    .unwrap()
    .unwrap();

    assert!(result.formatted_content.contains("forced 已运行"));
    assert_eq!(result.input_tokens, 12);
    assert_eq!(result.output_tokens, 34);
    assert!(!result.auto_applied);
    assert_eq!(result.output.deviations, vec!["forced 已运行"]);
    let _ = std::fs::remove_dir_all(cwd);
    let _ = std::fs::remove_dir_all(base_dir);
}

#[tokio::test]
async fn test_run_reflection_auto_apply_suggestions_writes_memory() {
    let cwd = temp_dir("reflection-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let base_dir = temp_dir("reflection-memory");
    let response = r#"{
            "suggested_memories": [
                {
                    "category": "decision",
                    "content": "后台 reflection 自动写入 memory",
                    "tags": ["reflection"],
                    "reason": "auto_apply_suggestions=true"
                }
            ]
        }"#;
    let client = build_client(response);
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 2;
    config.reflection.auto_apply_suggestions = true;

    let result = run_complete_reflection_with_base_dir(
        ReflectionRunMode::Forced,
        &config,
        &[share::message::Message::user("请记住这个决策")],
        &cwd,
        &client,
        "system prompt",
        base_dir.clone(),
        "zh",
    )
    .await
    .unwrap()
    .unwrap();
    let text = result.formatted_content.clone();
    let store = MemoryStore::new(
        &base_dir,
        storage::project_file_name_from_path(&cwd),
        config.max_entries,
        config.similarity_threshold,
    )
    .unwrap();
    let entries = store.list(Some(MemoryLayer::Project)).unwrap();

    assert!(text.contains("后台 reflection 自动写入 memory"));
    assert!(text.contains("已自动应用 Reflection：新增/合并 1 条记忆，标记 0 条过时记忆。"));
    assert!(result.auto_applied);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].category, MemoryCategory::Decision);
    assert_eq!(entries[0].content, "后台 reflection 自动写入 memory");
    assert_eq!(entries[0].source, MemorySource::Llm);
    let _ = std::fs::remove_dir_all(cwd);
    let _ = std::fs::remove_dir_all(base_dir);
}

#[tokio::test]
async fn test_run_reflection_auto_apply_false_does_not_write_memory() {
    let cwd = temp_dir("reflection-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let base_dir = temp_dir("reflection-memory");
    let response = r#"{
            "suggested_memories": [
                {
                    "category": "decision",
                    "content": "auto apply false 不写入",
                    "tags": ["reflection"],
                    "reason": "auto_apply_suggestions=false"
                }
            ]
        }"#;
    let client = build_client(response);
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = 2;
    config.reflection.auto_apply_suggestions = false;

    let text = run_reflection_with_base_dir(
        &config,
        2,
        &[share::message::Message::user("请只展示建议")],
        &cwd,
        &client,
        "system prompt",
        base_dir.clone(),
        "zh",
    )
    .await
    .unwrap();
    let store = MemoryStore::new(
        &base_dir,
        storage::project_file_name_from_path(&cwd),
        config.max_entries,
        config.similarity_threshold,
    )
    .unwrap();
    let entries = store.list(Some(MemoryLayer::Project)).unwrap();

    assert!(text.contains("auto apply false 不写入"));
    assert!(!text.contains("已自动应用 Reflection"));
    assert!(entries.is_empty());
    let _ = std::fs::remove_dir_all(cwd);
    let _ = std::fs::remove_dir_all(base_dir);
}

#[tokio::test]
async fn test_run_complete_reflection_empty_response_returns_err_empty_response() {
    let cwd = temp_dir("reflection-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let base_dir = temp_dir("reflection-memory");
    let client = build_client("   ");
    let config = share::config::MemoryConfig::default();

    let result = run_complete_reflection_with_base_dir(
        ReflectionRunMode::Forced,
        &config,
        &[share::message::Message::user("LLM 空响应")],
        &cwd,
        &client,
        "system prompt",
        base_dir.clone(),
        "zh",
    )
    .await;

    assert!(matches!(
        result,
        Err(crate::application::reflection::ReflectionError::EmptyResponse)
    ));
    let _ = std::fs::remove_dir_all(cwd);
    let _ = std::fs::remove_dir_all(base_dir);
}

#[tokio::test]
async fn test_run_complete_reflection_unparseable_returns_err_unparseable() {
    let cwd = temp_dir("reflection-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let base_dir = temp_dir("reflection-memory");
    let client = build_client("这不是 JSON 格式的反思结果");
    let config = share::config::MemoryConfig::default();

    let result = run_complete_reflection_with_base_dir(
        ReflectionRunMode::Forced,
        &config,
        &[share::message::Message::user("无法解析")],
        &cwd,
        &client,
        "system prompt",
        base_dir.clone(),
        "zh",
    )
    .await;

    assert!(matches!(
        result,
        Err(crate::application::reflection::ReflectionError::Unparseable(_))
    ));
    let _ = std::fs::remove_dir_all(cwd);
    let _ = std::fs::remove_dir_all(base_dir);
}
