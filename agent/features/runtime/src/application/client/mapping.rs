use sdk::{
    ConfigField, ConfigUpdateResult, ConfigView, ElementSpacingView, MarkdownSpacingModeView,
    MarkdownSpacingOverridesView, MemoryConfigView, ReflectionConfigView, SessionSummary,
};

pub fn config_snapshot_to_sdk(
    snapshot: &share::config::domain::snapshot::ConfigSnapshot,
) -> ConfigView {
    let overrides = snapshot.markdown_spacing_overrides();
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
        markdown_spacing: match snapshot.markdown_spacing_mode() {
            share::config::MarkdownSpacingMode::Normal => MarkdownSpacingModeView::Normal,
            share::config::MarkdownSpacingMode::Compact => MarkdownSpacingModeView::Compact,
        },
        markdown_spacing_overrides: MarkdownSpacingOverridesView {
            paragraph: overrides.paragraph.map(element_spacing_to_sdk),
            heading: overrides.heading.map(element_spacing_to_sdk),
            list: overrides.list.map(element_spacing_to_sdk),
            code_block: overrides.code_block.map(element_spacing_to_sdk),
            table: overrides.table.map(element_spacing_to_sdk),
            blockquote: overrides.blockquote.map(element_spacing_to_sdk),
        },
        context_size: snapshot.context_size(),
        logging_level: snapshot.logging_level().to_string(),
    }
}

fn element_spacing_to_sdk(value: share::config::ElementSpacingOverride) -> ElementSpacingView {
    ElementSpacingView {
        before: value.before.map(share::config::SpacingLines::get),
        after: value.after.map(share::config::SpacingLines::get),
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

pub(crate) fn skill_snapshot_to_sdk(
    snapshot: tools::SkillCatalogSnapshot,
) -> sdk::SkillsUpdatedEvent {
    sdk::SkillsUpdatedEvent {
        revision: snapshot.revision,
        skills: snapshot
            .skills
            .into_iter()
            .map(|skill| sdk::SkillView {
                name: skill.name().to_string(),
                aliases: skill.aliases().to_vec(),
                slash_command: skill.slash_command().map(str::to_string),
                slash_aliases: skill.slash_aliases().to_vec(),
                description: skill.description().to_string(),
                argument_hint: skill.argument_hint().map(str::to_string),
            })
            .collect(),
        slash_routes: snapshot
            .slash_routes
            .into_iter()
            .map(|route| sdk::SkillSlashRouteView {
                skill: route.skill,
                slash_command: route.slash_command,
                aliases: route.aliases,
                argument_hint: route.argument_hint,
            })
            .collect(),
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

pub(crate) fn session_summary_from_context(session: context::SessionListEntry) -> SessionSummary {
    SessionSummary {
        id: session.id,
        title: session.title,
        project: session.project,
        model: session.model,
        created_at: session.created_at,
        updated_at: session.updated_at,
        message_count: session.message_count,
        preview: session.preview,
        summary: session.summary,
    }
}

pub(crate) fn workspace_context_to_sdk(
    workspace: share::session_types::PersistedWorkspaceContext,
) -> sdk::WorkspaceContextView {
    sdk::WorkspaceContextView {
        path_base: workspace.path_base.into(),
        workspace_root: workspace.workspace_root.into(),
        context_stack: workspace
            .context_stack
            .into_iter()
            .map(|entry| sdk::WorkspaceStackEntryView {
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
                share::message::MessageSource::StopHook => sdk::ChatMessageSource::StopHook,
            },
            stop_hook: metadata.stop_hook.map(|payload| sdk::StopHookFeedbackView {
                summary: payload.summary,
                command: payload.command,
                exit_code: payload.exit_code,
                reason: payload.reason,
                stdout_preview: payload.stdout_preview,
                stderr_preview: payload.stderr_preview,
                stdout_truncated: payload.stdout_truncated,
                stderr_truncated: payload.stderr_truncated,
                output_file: payload.output_file,
            }),
        }),
        // input_id 不来自 share::Message；由 runtime→TUI 边界（UserMessagesAdded 事件）
        // 在 event.rs 处按 (InputId, Message) 元组注入（#507 修复）。
        input_id: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_mapping_preserves_stop_hook_payload() {
        let message = share::message::Message::stop_hook_feedback(
            "<system-reminder>blocked</system-reminder>",
            share::message::StopHookFeedback {
                summary: "blocked".to_string(),
                command: "check-agent-stop.sh".to_string(),
                exit_code: Some(2),
                reason: "exit code 2".to_string(),
                stdout_preview: "out".to_string(),
                stderr_preview: "err".to_string(),
                stdout_truncated: false,
                stderr_truncated: true,
                output_file: Some("/tmp/hook.txt".to_string()),
            },
        );

        let mapped = message_to_sdk(message);
        let payload = mapped.metadata.unwrap().stop_hook.unwrap();

        assert_eq!(payload.command, "check-agent-stop.sh");
        assert_eq!(payload.exit_code, Some(2));
        assert!(payload.stderr_truncated);
        assert_eq!(payload.output_file.as_deref(), Some("/tmp/hook.txt"));
    }

    #[test]
    fn config_snapshot_mapping_preserves_sdk_visible_fields() {
        let mut config = share::config::Config::default();
        config.model.name = "mapped/model".into();
        config.api.provider = Some("mapped-provider".into());
        config.api.key = Some("secret".into());
        config.permissions.mode = share::config::PermissionModeConfig::AllowAll;
        config.ui.markdown = false;
        config.ui.verbose = true;
        config.ui.markdown_spacing = share::config::MarkdownSpacingMode::Compact;
        config.ui.markdown_spacing_overrides.heading =
            Some(share::config::ElementSpacingOverride {
                before: Some(share::config::SpacingLines::new(1).unwrap()),
                after: Some(share::config::SpacingLines::new(2).unwrap()),
            });
        config.model.context_size = 42_000;
        config.logging.level = "debug".into();

        let view = config_snapshot_to_sdk(&share::config::domain::snapshot::ConfigSnapshot::new(
            config,
        ));

        assert_eq!(view.model_name, "mapped/model");
        assert_eq!(view.provider.as_deref(), Some("mapped-provider"));
        assert!(view.has_api_key);
        assert_eq!(view.permission_mode, "allow_all");
        assert!(!view.markdown);
        assert!(view.verbose);
        assert_eq!(view.markdown_spacing, sdk::MarkdownSpacingModeView::Compact);
        assert_eq!(
            view.markdown_spacing_overrides.heading,
            Some(sdk::ElementSpacingView {
                before: Some(1),
                after: Some(2),
            })
        );
        assert_eq!(view.context_size, 42_000);
        assert_eq!(view.logging_level, "debug");
    }

    #[test]
    fn config_change_mapping_preserves_fields_and_committed_view() {
        let mut config = share::config::Config::default();
        config.model.name = "changed/model".into();
        let result = config_change_to_sdk(config::ConfigChangeSet {
            cause: config::ConfigChangeCause::ClientUpdate,
            fields: vec![
                config::ConfigField::Model,
                config::ConfigField::PermissionMode,
            ],
            snapshot: share::config::domain::snapshot::ConfigSnapshot::new(config),
        });

        assert_eq!(
            result.changed_fields,
            vec![sdk::ConfigField::Model, sdk::ConfigField::PermissionMode]
        );
        assert_eq!(result.view.model_name, "changed/model");
    }
}
