use super::*;

#[test]
fn config_notice_reports_scope_effects_in_chinese() {
    let scopes = [
        ConfigApplicationScope::Run,
        ConfigApplicationScope::SessionRestartRequired,
    ];
    assert_eq!(
        config_reload_notice("zh", &scopes),
        "[config] 已重载。下次 Run 生效；重启 Session 后生效。"
    );
}

#[test]
fn config_notice_falls_back_to_english_for_unknown_lang() {
    assert_eq!(
        config_reload_notice("fr", &[ConfigApplicationScope::Run]),
        "[config] reloaded. effective next run."
    );
}

#[test]
fn config_notice_without_scopes_reports_reload_only() {
    assert_eq!(config_reload_notice("zh", &[]), "[config] 已重载。");
    assert_eq!(config_reload_notice("en", &[]), "[config] reloaded.");
}

#[test]
fn component_notice_reports_source_and_file_count() {
    assert_eq!(
        component_reload_notice("zh", ComponentReloadSource::Guidance, 2),
        "[guidance] 已重载（2 个文件）。"
    );
    assert_eq!(
        component_reload_notice("en", ComponentReloadSource::Instructions, 1),
        "[instructions] reloaded (1 file(s))."
    );
    assert_eq!(
        component_reload_notice("zh", ComponentReloadSource::Skills, 3),
        "[skills] 已重载（3 个文件）。"
    );
}
