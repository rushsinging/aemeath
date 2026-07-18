use sdk::{
    ConfigField, ConfigUpdateResult, ConfigView, MemoryConfigView, ReflectionConfigView,
    ReflectionMemorySuggestionView, ReflectionOutputView, SessionSummary, SkillView,
    WorkspaceContextView, WorkspaceStackEntryView,
};

use context::skill::Skill;
use share::memory::{MemoryCategory, MemoryLayer};

pub(crate) fn config_snapshot_to_sdk(
    snapshot: &share::config::domain::snapshot::ConfigSnapshot,
) -> ConfigView {
    ConfigView {
        model_name: snapshot.model_name().to_string(),
        provider: snapshot.provider().map(str::to_string),
        has_api_key: snapshot.api_key().is_some(),
        permission_mode: match snapshot.permission_mode() {
            share::config::PermissionModeConfig::Ask => "ask",
            share::config::PermissionModeConfig::AutoRead => "auto_read",
            share::config::PermissionModeConfig::AllowAll => "allow_all",
        }
        .to_string(),
        markdown: snapshot.markdown(),
        verbose: snapshot.verbose(),
        context_size: snapshot.context_size(),
        logging_level: snapshot.logging_level().to_string(),
    }
}

pub(crate) fn config_change_to_sdk(change: config::ConfigChangeSet) -> ConfigUpdateResult {
    ConfigUpdateResult {
        changed_fields: change
            .fields
            .into_iter()
            .map(|field| match field {
                config::ConfigField::Model => ConfigField::Model,
                config::ConfigField::PermissionMode => ConfigField::PermissionMode,
                config::ConfigField::Memory => ConfigField::Memory,
            })
            .collect(),
        view: config_snapshot_to_sdk(&change.snapshot),
    }
}

pub(crate) fn memory_config_to_sdk(config: share::config::MemoryConfig) -> MemoryConfigView {
    MemoryConfigView {
        enabled: config.enabled,
        max_entries: config.max_entries,
        similarity_threshold: config.similarity_threshold as f32,
        reflection: ReflectionConfigView {
            enabled: config.reflection.enabled,
            interval_turns: config.reflection.interval_turns,
            auto_apply_suggestions: config.reflection.auto_apply_suggestions,
        },
    }
}

pub(crate) fn skill_to_sdk(skill: Skill) -> SkillView {
    SkillView {
        name: skill.name,
        aliases: skill.aliases,
        description: Some(skill.description),
        content: skill.content,
        source: Some(skill.source_path.display().to_string()),
    }
}

pub(crate) fn reflection_output_to_sdk_with_content(
    output: crate::application::reflection::ReflectionOutput,
    content: String,
    input_tokens: u32,
    output_tokens: u32,
    auto_applied: bool,
) -> ReflectionOutputView {
    ReflectionOutputView {
        content,
        input_tokens,
        output_tokens,
        suggested_memories: output
            .suggested_memories
            .into_iter()
            .map(|memory| ReflectionMemorySuggestionView {
                content: memory.content,
                layer: memory_layer_to_sdk(memory.layer).to_string(),
                category: memory_category_to_sdk(memory.category).to_string(),
                tags: memory.tags,
            })
            .collect(),
        outdated_memories: output.outdated_memories,
        auto_applied,
    }
}

fn memory_layer_to_sdk(layer: MemoryLayer) -> &'static str {
    match layer {
        MemoryLayer::Global => "global",
        MemoryLayer::Project => "project",
    }
}

fn memory_category_to_sdk(category: MemoryCategory) -> &'static str {
    match category {
        MemoryCategory::Fact => "fact",
        MemoryCategory::Decision => "decision",
        MemoryCategory::Preference => "preference",
        MemoryCategory::Pattern => "pattern",
        MemoryCategory::Pitfall => "pitfall",
    }
}

pub(crate) fn session_summary_from_runtime(session: context::session::Session) -> SessionSummary {
    let preview = session
        .messages
        .iter()
        .find(|m| m.role == share::message::Role::User)
        .map(|m| m.text_content())
        .and_then(|text| {
            let first_line = text.lines().next().unwrap_or("").trim();
            if first_line.is_empty() {
                None
            } else {
                Some(first_line.chars().take(50).collect())
            }
        });
    let summary = session.summary();
    SessionSummary {
        id: session.id,
        title: session.metadata.title,
        project: session.metadata.project,
        model: session.metadata.model,
        created_at: session.created_at,
        updated_at: session.updated_at,
        message_count: session.messages.len(),
        preview,
        summary,
    }
}

pub(crate) fn workspace_context_to_sdk(
    workspace: context::session::PersistedWorkspaceContext,
) -> WorkspaceContextView {
    WorkspaceContextView {
        path_base: workspace.path_base.into(),
        workspace_root: workspace.workspace_root.into(),
        context_stack: workspace
            .context_stack
            .into_iter()
            .map(|entry| WorkspaceStackEntryView {
                path_base: entry.path_base.into(),
                workspace_root: entry.workspace_root.into(),
            })
            .collect(),
    }
}

pub(crate) fn message_to_sdk(message: share::message::Message) -> sdk::ChatMessage {
    sdk::ChatMessage {
        role: match message.role {
            share::message::Role::User => "user".to_string(),
            share::message::Role::Assistant => "assistant".to_string(),
        },
        // share::ContentBlock 与 sdk::ContentBlock 同形（serde 成同一 JSON），经 round-trip 映射。
        content: serde_json::from_value(serde_json::to_value(&message.content).unwrap_or_default())
            .unwrap_or_default(),
        metadata: message.metadata.map(|metadata| sdk::ChatMessageMetadata {
            source: match metadata.source {
                share::message::MessageSource::User => sdk::ChatMessageSource::User,
                share::message::MessageSource::SystemGenerated => {
                    sdk::ChatMessageSource::SystemGenerated
                }
            },
        }),
        // input_id 不来自 share::Message；由 runtime→TUI 边界（UserMessagesAdded 事件）
        // 在 event.rs 处按 (InputId, Message) 元组注入（#507 修复）。
        input_id: None,
    }
}

pub(crate) fn message_from_sdk(message: sdk::ChatMessage) -> share::message::Message {
    let role = match message.role.as_str() {
        "assistant" => share::message::Role::Assistant,
        _ => share::message::Role::User,
    };
    let content =
        serde_json::from_value(serde_json::to_value(&message.content).unwrap_or_default())
            .unwrap_or_else(|_| {
                vec![share::message::ContentBlock::Text {
                    text: String::new(),
                }]
            });
    let metadata = message
        .metadata
        .map(|metadata| share::message::MessageMetadata {
            source: match metadata.source {
                sdk::ChatMessageSource::User => share::message::MessageSource::User,
                sdk::ChatMessageSource::SystemGenerated => {
                    share::message::MessageSource::SystemGenerated
                }
            },
        });
    share::message::Message {
        role,
        content,
        metadata,
    }
}

pub(crate) fn model_display(source_key: &str, model_name: &str, model_id: &str) -> String {
    let display_name = if model_name.is_empty() {
        model_id
    } else {
        model_name
    };
    format!("{}/{}", source_key, display_name)
}
