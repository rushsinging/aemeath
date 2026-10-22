use super::*;

#[test]
fn message_mapping_preserves_hook_notice() {
    let message = share::message::Message::hook_notice(
        "<system-reminder>blocked</system-reminder>",
        share::message::HookNotice {
            point: "Stop".to_string(),
            kind: share::message::HookNoticeKind::Blocked,
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
    let notice = mapped.metadata.unwrap().hook_notice.unwrap();

    assert_eq!(notice.point, "Stop");
    assert_eq!(notice.command, "check-agent-stop.sh");
    assert_eq!(notice.exit_code, Some(2));
    assert!(notice.stderr_truncated);
    assert_eq!(notice.output_file.as_deref(), Some("/tmp/hook.txt"));
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
    config.ui.markdown_spacing_overrides.heading = Some(share::config::ElementSpacingOverride {
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
    let result = config_change_to_sdk(config::ConfigChangeData {
        cause: config::ConfigChangeCauseData::ClientUpdate,
        fields: vec![
            config::ConfigFieldData::Model,
            config::ConfigFieldData::PermissionMode,
        ],
        snapshot: share::config::domain::snapshot::ConfigSnapshot::new(config),
    });

    assert_eq!(
        result.changed_fields,
        vec![
            sdk::ConfigFieldData::Model,
            sdk::ConfigFieldData::PermissionMode
        ]
    );
    assert_eq!(result.view.model_name, "changed/model");
}
