use sdk::{
    ConfigFieldData, ConfigUpdateResult, ConfigView, ElementSpacingView, MarkdownSpacingModeView,
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

pub(crate) fn config_change_to_sdk(change: config::ConfigChangeData) -> ConfigUpdateResult {
    ConfigUpdateResult {
        changed_fields: change
            .fields
            .into_iter()
            .map(|field| match field {
                config::ConfigFieldData::Model => ConfigFieldData::Model,
                config::ConfigFieldData::PermissionMode => ConfigFieldData::PermissionMode,
                config::ConfigFieldData::Memory => ConfigFieldData::Memory,
            })
            .collect(),
        view: config_snapshot_to_sdk(&change.snapshot),
    }
}

pub(crate) fn skill_snapshot_to_sdk(
    snapshot: tools::published::skill::SkillCatalogSnapshot,
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
            interval_runs: config.reflection.interval_runs,
            auto_apply_suggestions: config.reflection.auto_apply_suggestions,
        },
    }
}

pub(crate) fn session_summary_from_context(
    session: context::SessionListEntryData,
) -> SessionSummary {
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

pub(crate) fn map_finalize_cause_to_sdk(
    cause: context::FinalizeCause,
) -> sdk::ResumedStepFinalizeCause {
    match cause {
        context::FinalizeCause::Completed => sdk::ResumedStepFinalizeCause::Completed,
        context::FinalizeCause::UserCancelledStep => {
            sdk::ResumedStepFinalizeCause::UserCancelledStep
        }
        context::FinalizeCause::RunTerminated => sdk::ResumedStepFinalizeCause::RunTerminated,
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
                share::message::MessageSource::Hook => sdk::ChatMessageSource::Hook,
                share::message::MessageSource::SkillRequest => sdk::ChatMessageSource::SkillRequest,
            },
            system_reminder: metadata.system_reminder,
            hook_notice: metadata.hook_notice.map(|notice| sdk::HookNoticeView {
                point: notice.point,
                kind: match notice.kind {
                    share::message::HookNoticeKind::Blocked => sdk::HookNoticeKindView::Blocked,
                    share::message::HookNoticeKind::Failed => sdk::HookNoticeKindView::Failed,
                    share::message::HookNoticeKind::Info => sdk::HookNoticeKindView::Info,
                },
                summary: notice.summary,
                command: notice.command,
                exit_code: notice.exit_code,
                reason: notice.reason,
                stdout_preview: notice.stdout_preview,
                stderr_preview: notice.stderr_preview,
                stdout_truncated: notice.stdout_truncated,
                stderr_truncated: notice.stderr_truncated,
                output_file: notice.output_file,
            }),
            skill_request: metadata
                .skill_request
                .map(|payload| sdk::SkillRequestMetadataView {
                    skill: payload.skill,
                    arguments: payload.arguments,
                    raw_input: payload.raw_input,
                }),
        }),
        // input_id 不来自 share::Message；由 runtime→TUI 边界（UserMessagesAdded 事件）
        // 在 event.rs 处按 (InputId, Message) 元组注入（#507 修复）。
        input_id: None,
    }
}

pub(crate) fn display_history_window_to_sdk(
    window: context::api::DisplayHistoryStepWindowData,
) -> sdk::DisplayHistoryWindow {
    sdk::DisplayHistoryWindow {
        session_id: window.session_id().to_string(),
        generation_revision: window.generation_revision(),
        steps: window
            .steps()
            .iter()
            .map(|member| {
                let step = member.step();
                sdk::ResumedSessionStep {
                    run_id: member.cursor().run_id.clone(),
                    step_id: member.cursor().step_id.clone(),
                    messages: step
                        .accepted_input
                        .iter()
                        .flat_map(|input| input.messages.iter())
                        .chain(
                            step.outcome
                                .iter()
                                .flat_map(|outcome| outcome.messages.iter()),
                        )
                        .cloned()
                        .map(message_to_sdk)
                        .collect(),
                    finalize_cause: step
                        .outcome
                        .as_ref()
                        .map(|outcome| map_finalize_cause_to_sdk(outcome.finalize_cause)),
                    duration_ms: step
                        .outcome
                        .as_ref()
                        .and_then(|outcome| outcome.duration_ms),
                }
            })
            .collect(),
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
#[path = "mapping_tests.rs"]
mod tests;
