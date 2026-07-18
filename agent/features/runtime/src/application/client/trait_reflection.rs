use memory::api::{MemoryCategory, MemoryLayer, MemorySuggestion, ReflectionOutput};
use sdk::{ReflectionOutputView, SdkError};

use super::accessors::AgentClientImpl;

type Result<T> = std::result::Result<T, SdkError>;

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
    let reflection = memory::api::ReflectionEngine;
    let result = crate::application::reflection::run_complete_reflection(
        crate::application::reflection::ReflectionRunMode::Forced,
        &me.inner.context.resources.memory_config,
        &runtime_messages,
        client.as_ref(),
        &me.inner.context.resources.system_prompt_text,
        &me.inner.context.resources.language,
        me.inner.context.resources.memory.as_ref(),
        &reflection,
    )
    .await
    .map_err(|error| {
        let message = match error {
            crate::application::reflection::ReflectionError::LlmCall(detail) => {
                format!("反思 LLM 调用失败：{detail}")
            }
            crate::application::reflection::ReflectionError::EmptyResponse => {
                "LLM 未返回任何反思内容".to_string()
            }
            crate::application::reflection::ReflectionError::Unparseable(detail) => {
                format!("LLM 返回的内容无法解析为反思 JSON：{detail}")
            }
        };
        SdkError::Internal(message)
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
    apply_reflection_with_memory(
        &me.inner.context.resources.memory_config,
        me.inner.context.resources.memory.as_ref(),
        output,
    )
    .await
}

async fn apply_reflection_with_memory(
    config: &share::config::MemoryConfig,
    memory: &dyn memory::api::MemoryPort,
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
    let applied = memory
        .apply_reflection(&reflection_output)
        .await
        .map_err(|error| SdkError::Internal(format!("应用 Reflection 失败：{error}")))?;

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

fn reflection_output_from_sdk(output: ReflectionOutputView) -> Result<ReflectionOutput> {
    Ok(ReflectionOutput {
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
    suggestion: sdk::ReflectionMemorySuggestionView,
) -> Result<MemorySuggestion> {
    Ok(MemorySuggestion {
        layer: parse_memory_layer(&suggestion.layer)?,
        category: parse_memory_category(&suggestion.category)?,
        content: suggestion.content,
        tags: suggestion.tags,
        reason: String::new(),
    })
}

fn parse_memory_layer(value: &str) -> Result<MemoryLayer> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        SdkError::Internal(format!(
            "无法应用 Reflection：未知 memory layer `{value}`。"
        ))
    })
}

fn parse_memory_category(value: &str) -> Result<MemoryCategory> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        SdkError::Internal(format!(
            "无法应用 Reflection：未知 memory category `{value}`。"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory::api::{MemoryCategory, MemoryLayer, MemoryPort};

    fn reflection_view(
        suggestions: Vec<sdk::ReflectionMemorySuggestionView>,
        outdated_memories: Vec<String>,
        auto_applied: bool,
    ) -> ReflectionOutputView {
        ReflectionOutputView {
            content: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            suggested_memories: suggestions,
            outdated_memories,
            auto_applied,
        }
    }

    fn suggestion_view() -> sdk::ReflectionMemorySuggestionView {
        sdk::ReflectionMemorySuggestionView {
            content: "显式 apply 写入记忆".to_string(),
            layer: "project".to_string(),
            category: "decision".to_string(),
            tags: vec!["reflection".to_string()],
        }
    }

    #[tokio::test]
    async fn apply_uses_active_memory_contract() {
        let memory =
            memory::api::InMemoryMemory::new(memory::api::MemoryPolicy::default()).unwrap();
        let message = apply_reflection_with_memory(
            &share::config::MemoryConfig::default(),
            &memory,
            reflection_view(vec![suggestion_view()], Vec::new(), false),
        )
        .await
        .unwrap();

        let entries = memory.list(Some(MemoryLayer::Project));
        assert!(message.contains("新增/合并 1 条记忆"));
        assert!(message.contains("标记 0 条过时记忆"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "显式 apply 写入记忆");
        assert_eq!(entries[0].category, MemoryCategory::Decision);
        assert_eq!(entries[0].tags, vec!["reflection"]);
    }

    #[tokio::test]
    async fn apply_short_circuits_without_calling_memory() {
        let memory = memory::api::NoOpMemory;
        let config = share::config::MemoryConfig::default();

        let auto_applied = apply_reflection_with_memory(
            &config,
            &memory,
            reflection_view(vec![suggestion_view()], Vec::new(), true),
        )
        .await
        .unwrap();
        let empty = apply_reflection_with_memory(
            &config,
            &memory,
            reflection_view(Vec::new(), Vec::new(), false),
        )
        .await
        .unwrap();

        assert!(auto_applied.contains("已自动应用"));
        assert!(empty.contains("没有可应用"));
    }

    #[tokio::test]
    async fn apply_rejects_disabled_or_invalid_sdk_input() {
        let memory = memory::api::NoOpMemory;
        let disabled = share::config::MemoryConfig {
            enabled: false,
            ..Default::default()
        };
        let disabled_error = apply_reflection_with_memory(
            &disabled,
            &memory,
            reflection_view(vec![suggestion_view()], Vec::new(), false),
        )
        .await
        .unwrap_err();
        assert!(disabled_error.to_string().contains("memory.enabled=false"));

        let mut invalid = suggestion_view();
        invalid.layer = "session".to_string();
        let invalid_error = apply_reflection_with_memory(
            &share::config::MemoryConfig::default(),
            &memory,
            reflection_view(vec![invalid], Vec::new(), false),
        )
        .await
        .unwrap_err();
        assert!(invalid_error.to_string().contains("未知 memory layer"));
    }
}
