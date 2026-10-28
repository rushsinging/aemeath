//! config 热载对外事件构造。
//!
//! 两条检测入口 MUST 共用本构造，保证任何真正生效的配置重载都有且只有一次
//! 可见回显：
//! - turn 边界 `refresh_if_sources_changed`（`loop_phases`）；
//! - 即时查询（`/model` 的 `ListModels`）前的刷新（`immediate_query`）——
//!   该路径会提交新快照，若不在此回显，之后 turn 边界只会看到 `Unchanged`，
//!   用户永远看不到「已重载」。

use crate::application::loop_engine::chat::RuntimeStreamEvent;
use share::config::domain::scope::ConfigApplicationScope;

/// 构造 `[ConfigReloaded, SystemMessage]` 事件序列。
///
/// `ConfigReloaded` 携带 `config:scope:*` key 与提交后的视图；`SystemMessage`
/// 是 i18n 可见回显（只报已重载 + 生效边界，NEVER 做字段级 diff）。
pub(crate) fn config_reload_events(
    language: &str,
    scopes: &[ConfigApplicationScope],
    view: sdk::ConfigView,
) -> [RuntimeStreamEvent; 2] {
    let mut changed_keys = vec!["config:reloaded".to_string()];
    changed_keys.extend(
        scopes
            .iter()
            .map(|scope| format!("config:scope:{}", scope.as_str())),
    );
    [
        RuntimeStreamEvent::ConfigReloaded { changed_keys, view },
        RuntimeStreamEvent::SystemMessage(
            share::i18n::runtime::config_reload::config_reload_notice(language, scopes),
        ),
    ]
}

#[cfg(test)]
#[path = "reload_notification_tests.rs"]
mod tests;
