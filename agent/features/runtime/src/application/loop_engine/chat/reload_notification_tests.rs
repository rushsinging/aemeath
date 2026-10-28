use super::*;

#[test]
fn config_reload_events_carry_scope_keys_and_localized_notice() {
    let events = config_reload_events(
        "zh",
        &[ConfigApplicationScope::Run],
        sdk::ConfigView::default(),
    );

    match &events[0] {
        RuntimeStreamEvent::ConfigReloaded { changed_keys, .. } => assert_eq!(
            changed_keys,
            &vec![
                "config:reloaded".to_string(),
                "config:scope:run".to_string()
            ],
            "ConfigReloaded 必须同时携带 reloaded 标记与 scope key"
        ),
        other => panic!("首个事件必须是 ConfigReloaded，实际: {other:?}"),
    }
    match &events[1] {
        RuntimeStreamEvent::SystemMessage(text) => {
            assert_eq!(text, "[config] 已重载。下次 Run 生效。")
        }
        other => panic!("第二个事件必须是可见 SystemMessage，实际: {other:?}"),
    }
}
