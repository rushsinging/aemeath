//! adapters 层测试（自 adapters.rs 迁出，行为等价）。
use super::*;

struct FakeEnv(HashMap<String, String>);

impl EnvSource for FakeEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

#[test]
fn env_adapter_prefers_aemeath_key_and_maps_driver_keys() {
    let source = FakeEnv(HashMap::from([
        ("AEMEATH_API_KEY".into(), "aemeath-key".into()),
        ("LLM_API_KEY".into(), "llm-key".into()),
        ("ANTHROPIC_API_KEY".into(), "anthropic-key".into()),
        ("AEMEATH_MODEL".into(), "anthropic/model".into()),
    ]));
    let patch = EnvAdapter::read(&source);
    assert_eq!(patch.api.unwrap().key.as_deref(), Some("aemeath-key"));
    let models = patch.models.unwrap();
    assert_eq!(
        models.provider_api_keys.unwrap()["anthropic"],
        "anthropic-key"
    );
    assert_eq!(models.fallback_api_key.as_deref(), Some("aemeath-key"));
}

#[test]
fn env_adapter_ignores_invalid_and_retired_reasoning_env() {
    let source = FakeEnv(HashMap::from([
        ("AEMEATH_MAX_TOKENS".into(), "0".into()),
        ("AEMEATH_MAX_TOOL_CONCURRENCY".into(), "bad".into()),
        ("AEMEATH_MAX_REASONING".into(), "high".into()),
    ]));
    let patch = EnvAdapter::read(&source);
    assert!(patch.model.is_none());
    assert!(patch.tools.is_none());
}

#[test]
fn env_adapter_maps_supported_scalar_values() {
    let source = FakeEnv(HashMap::from([
        ("AEMEATH_BASE_URL".into(), "https://example.test".into()),
        ("AEMEATH_MAX_TOKENS".into(), "4096".into()),
        ("AEMEATH_CONTEXT_SIZE".into(), "128000".into()),
        ("AEMEATH_PERMISSION_MODE".into(), "allow_all".into()),
        ("AEMEATH_MAX_TOOL_CONCURRENCY".into(), "7".into()),
        ("AEMEATH_MAX_AGENT_CONCURRENCY".into(), "3".into()),
        ("AEMEATH_VERBOSE".into(), "1".into()),
        ("NO_COLOR".into(), "1".into()),
        ("AEMEATH_LOG_LEVEL".into(), "debug".into()),
        ("AEMEATH_WORKTREES_DIR".into(), "/custom/worktrees".into()),
    ]));
    let patch = EnvAdapter::read(&source);
    assert_eq!(
        patch.api.unwrap().base_url.as_deref(),
        Some("https://example.test")
    );
    let model = patch.model.unwrap();
    assert_eq!(model.max_tokens, Some(4096));
    assert_eq!(model.context_size, Some(128000));
    assert_eq!(
        patch.permissions.unwrap().mode,
        Some(PermissionModeConfig::AllowAll)
    );
    assert_eq!(patch.tools.unwrap().max_concurrency, Some(7));
    assert_eq!(patch.agents.unwrap().max_concurrency, Some(3));
    let ui = patch.ui.unwrap();
    assert_eq!(ui.verbose, Some(true));
    assert_eq!(ui.color, Some(false));
    assert_eq!(patch.logging.unwrap().level.as_deref(), Some("debug"));
    assert_eq!(
        patch.storage.unwrap().worktrees_dir,
        Some(std::path::PathBuf::from("/custom/worktrees"))
    );
}

#[test]
fn env_adapter_omits_storage_patch_without_worktrees_dir() {
    let source = FakeEnv(HashMap::from([(
        "AEMEATH_LOG_LEVEL".into(),
        "debug".into(),
    )]));

    let patch = EnvAdapter::read(&source);

    assert!(patch.storage.is_none());
}

#[test]
fn config_validator_rejects_invalid_user_agent() {
    let mut config = share::config::Config::default();
    config.api.user_agent = "invalid\nuser-agent".to_string();

    assert_eq!(
        ConfigValidator::validate(&config),
        Err(ConfigAdapterError::Invalid)
    );
}

#[test]
fn config_validator_rejects_zero_concurrency() {
    let mut config = share::config::Config::default();
    config.tools.max_concurrency = 0;
    assert_eq!(
        ConfigValidator::validate(&config),
        Err(ConfigAdapterError::Invalid)
    );
}

#[test]
fn config_validator_unknown_model_reports_detail_with_available_sources() {
    let mut config = share::config::Config::default();
    config.models.default = "Wanaka/gpt-5.6-sol".into();
    config.models.providers.insert(
        "OmniRoute".into(),
        share::config::models::ProviderModelsConfig {
            driver: "openai".into(),
            ..Default::default()
        },
    );
    config.models.providers.insert(
        "Zhipu".into(),
        share::config::models::ProviderModelsConfig {
            driver: "zhipu".into(),
            ..Default::default()
        },
    );

    let error = ConfigValidator::validate(&config).unwrap_err();
    // Display 直接透传根因，不再叠加「模型选择无效」等冗余包装。
    let display = error.to_string();
    let ConfigAdapterError::InvalidModel { detail } = error else {
        panic!("未知模型来源应返回带详情的 InvalidModel，实际：{error:?}");
    };
    // 中文诊断信息必须保留根因与可用来源，不能只剩 "Invalid"。
    assert!(
        detail.contains("Wanaka"),
        "detail 应包含被查询的来源：{detail}"
    );
    assert!(
        detail.contains("OmniRoute") && detail.contains("Zhipu"),
        "detail 应列出可用来源：{detail}"
    );
    assert!(
        detail.contains("未找到模型来源"),
        "detail 应为中文可读诊断：{detail}"
    );
    assert_eq!(display, detail);
}

#[test]
fn claude_translator_maps_deny_list_without_inferring_mode_and_filters_blank_hooks() {
    let patch = ClaudeTranslator::translate(
            r#"{"permissions":{"allow":["Read"],"deny":["Bash"]},"hooks":{"Stop":[{"matcher":"*","hooks":[{"command":"   "},{"command":"echo ok","timeout":9}]}]}}"#,
        )
        .unwrap();
    let permissions = patch.permissions.unwrap();
    // #1469：兼容层只映射列表，不推断 mode——显式配置的 mode 是唯一真相。
    assert_eq!(permissions.mode, None);
    assert_eq!(permissions.auto_approve, Some(vec!["Read".to_string()]));
    assert_eq!(permissions.deny, Some(vec!["Bash".to_string()]));
    let hooks = patch.hooks.unwrap();
    let entries = &hooks.events[&share::config::hooks::HookEvent::Stop];
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "echo ok");
    assert_eq!(entries[0].timeout, 9);
}

#[test]
fn env_adapter_does_not_read_retired_logging_output_env() {
    struct RejectRetiredLoggingEnv;

    impl EnvSource for RejectRetiredLoggingEnv {
        fn get(&self, name: &str) -> Option<String> {
            assert_ne!(name, "AEMEATH_LOG_STDERR");
            None
        }
    }

    assert!(EnvAdapter::read(&RejectRetiredLoggingEnv).is_empty());
}

#[test]
fn cli_adapter_only_maps_explicit_values() {
    let empty = CliArgsAdapter::read(&CliConfigInput::default());
    assert!(empty.is_empty());
    let patch = CliArgsAdapter::read(&CliConfigInput {
        model: Some("local/model".into()),
        max_tool_concurrency: Some(7),
        ..Default::default()
    });
    assert_eq!(
        patch.models.unwrap().default.as_deref(),
        Some("local/model")
    );
    assert_eq!(patch.tools.unwrap().max_concurrency, Some(7));
}

#[test]
fn claude_translator_maps_hooks_model_and_permissions() {
    let patch = ClaudeTranslator::translate(
            r#"{"model":"local/model","permissions":{"allow":["Read"]},"hooks":{"Stop":[{"matcher":"","hooks":[{"command":"echo ok"}]}]}}"#,
        )
        .unwrap();
    assert_eq!(
        patch.models.unwrap().default.as_deref(),
        Some("local/model")
    );
    let permissions = patch.permissions.unwrap();
    // #1469：allow 列表只映射 auto_approve，不推断 mode。
    assert_eq!(permissions.mode, None);
    assert_eq!(permissions.auto_approve, Some(vec!["Read".to_string()]));
    assert_eq!(patch.hooks.unwrap().events.len(), 1);
}

#[test]
fn claude_translator_without_allow_or_deny_keeps_permissions_empty() {
    let patch = ClaudeTranslator::translate(r#"{"model":"local/model"}"#).unwrap();
    assert!(patch.permissions.is_none());
}

#[tokio::test]
async fn explicit_global_mode_is_not_overridden_by_claude_settings_deny() {
    // #1469 场景：全局显式 allow_all + 项目 .claude/settings.json（deny 非空），
    // 兼容层不得把 mode 覆盖为 Ask。
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("aemeath.json");
    let claude = dir.path().join(".claude/settings.json");
    tokio::fs::create_dir_all(dir.path().join(".claude"))
        .await
        .unwrap();
    tokio::fs::write(&global, r#"{"permissions":{"mode":"allow_all"}}"#)
        .await
        .unwrap();
    tokio::fs::write(&claude, r#"{"permissions":{"deny":["Artifact"]}}"#)
        .await
        .unwrap();

    let mut chain = share::config::domain::merge::PriorityChain::new();
    chain.push(FileAdapter::read(&global).await.unwrap().unwrap());
    chain.push(
        CompatibilityAdapter::read_one(&claude)
            .await
            .unwrap()
            .unwrap(),
    );
    let config = chain.merge(share::config::Config::default());

    assert_eq!(
        config.permissions.mode,
        share::config::PermissionModeConfig::AllowAll,
        "显式全局 mode 必须保持，不被 .claude/settings.json deny 列表覆盖"
    );
}

#[tokio::test]
async fn file_adapter_distinguishes_absent_and_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(FileAdapter::read(&dir.path().join("missing.json"))
        .await
        .unwrap()
        .is_none());
    let invalid = dir.path().join("invalid.json");
    tokio::fs::write(&invalid, "not-json").await.unwrap();
    assert!(matches!(
        FileAdapter::read(&invalid).await,
        Err(ConfigAdapterError::Parse)
    ));
}

#[tokio::test]
async fn compatibility_paths_are_applied_in_stable_order() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join(".claude-a/settings.json");
    let second = dir.path().join(".claude-z/settings.json");
    tokio::fs::create_dir_all(first.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::create_dir_all(second.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&first, r#"{"model":"first/model"}"#)
        .await
        .unwrap();
    tokio::fs::write(&second, r#"{"model":"second/model"}"#)
        .await
        .unwrap();
    let patches = CompatibilityAdapter::read_paths(vec![second, first])
        .await
        .unwrap();
    assert_eq!(patches.len(), 2);
    assert_eq!(
        patches[0].models.as_ref().unwrap().default.as_deref(),
        Some("first/model")
    );
    assert_eq!(
        patches[1].models.as_ref().unwrap().default.as_deref(),
        Some("second/model")
    );
}

#[tokio::test]
async fn native_store_round_trips_patch_and_maps_commit_warning() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let store = NativeConfigStore::new(storage);
    let bytes = br#"{"models":{"default":"local/model"}}"#;
    assert_eq!(store.write_override("project", bytes).await.unwrap(), None);
    let patch = store.read_override("project").await.unwrap().unwrap();
    assert_eq!(
        patch.models.unwrap().default.as_deref(),
        Some("local/model")
    );
}

#[tokio::test]
async fn native_store_contract_reports_missing_and_invalid_payload() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let store = NativeConfigStore::new(storage);
    assert!(store.read_override("missing").await.unwrap().is_none());
    store.write_override("invalid", b"not-json").await.unwrap();
    assert!(matches!(
        store.read_override("invalid").await,
        Err(ConfigAdapterError::Parse)
    ));
    assert!(matches!(
        store.read_override("bad/key").await,
        Err(ConfigAdapterError::Invalid)
    ));
}

#[test]
fn format_detection_rejects_unknown_settings() {
    assert_eq!(
        CompatibilityAdapter::detect_format(Path::new("settings.json"), "{}"),
        ConfigFormat::Unknown
    );
}
