use super::{PathBuf, SessionState};

fn empty_state() -> SessionState {
    SessionState {
        session_id: "sess-1".into(),
        cwd: PathBuf::from("/tmp"),
        current_model_display: String::new(),
        ..SessionState::default()
    }
}

#[test]
fn test_session_state_holds_session_id() {
    assert_eq!(empty_state().session_id, "sess-1");
}

#[test]
fn remember_and_sync_rewrites_display_after_name_change() {
    let mut state = empty_state();
    state.current_model_display = "Volcengine/DeepSeek V4.1 Flash (Ark)".into();
    state.cache_models(vec![sdk::ModelSummary {
        provider: "Volcengine".into(),
        id: "deepseek-v4.1-flash".into(),
        name: "DeepSeek V4.1 Flash (Ark)".into(),
        context_window: 128_000,
        max_tokens: 8_192,
    }]);

    state.remember_bound_model_from_cache();
    assert_eq!(
        state
            .bound_model
            .as_ref()
            .map(|(p, i)| (p.as_str(), i.as_str())),
        Some(("Volcengine", "deepseek-v4.1-flash"))
    );
    state.invalidate_cached_models();

    let refreshed = state.sync_current_model_display_from_list(&[sdk::ModelSummary {
        provider: "Volcengine".into(),
        id: "deepseek-v4.1-flash".into(),
        name: "DeepSeek V4.1 Flash".into(),
        context_window: 128_000,
        max_tokens: 8_192,
    }]);
    assert_eq!(refreshed.as_deref(), Some("Volcengine/DeepSeek V4.1 Flash"));
    assert_eq!(
        state.current_model_display,
        "Volcengine/DeepSeek V4.1 Flash"
    );
}

#[test]
fn invalidate_cached_models_returns_to_unloaded_state() {
    let mut state = SessionState {
        current_model_display: String::new(),
        ..Default::default()
    };
    state.cache_models(vec![sdk::ModelSummary {
        provider: "Volcengine".to_string(),
        id: "kimi-k3".to_string(),
        name: "Kimi K3".to_string(),
        context_window: 128_000,
        max_tokens: 8_192,
    }]);
    assert!(state.cached_models.is_some());

    state.invalidate_cached_models();
    assert!(
        state.cached_models.is_none(),
        "invalidate must restore unloaded semantics so /model waits for ModelList"
    );
}

#[test]
fn cache_models_distinguishes_unloaded_from_confirmed_empty() {
    let mut state = empty_state();
    assert!(
        state.cached_models.is_none(),
        "初始状态必须是未加载（None），区别于确认为空"
    );

    state.cache_models(Vec::new());

    assert_eq!(
        state.cached_models,
        Some(Vec::new()),
        "收到空 ModelList 后必须落为 Some(空)，表示 runtime 确认无模型"
    );
}

#[test]
fn cache_models_replaces_previous_entries() {
    let mut state = empty_state();
    state.cache_models(vec![sdk::ModelSummary {
        provider: "anthropic".into(),
        id: "claude-3".into(),
        name: "Claude 3".into(),
        context_window: 200_000,
        max_tokens: 8_000,
    }]);
    state.cache_models(vec![sdk::ModelSummary {
        provider: "openai".into(),
        id: "gpt-5".into(),
        name: "GPT-5".into(),
        context_window: 400_000,
        max_tokens: 16_000,
    }]);

    let cached = state.cached_models.expect("缓存应已回填");
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].provider, "openai");
}

#[test]
fn cache_sessions_replaces_previous_entries() {
    let mut state = empty_state();
    state.cache_sessions(vec![("s-1".into(), "first".into())]);
    state.cache_sessions(vec![("s-2".into(), "second".into())]);

    assert_eq!(
        state.cached_sessions,
        vec![("s-2".to_string(), "second".to_string())]
    );
}
