//! Config / 组件热载回显文案（TUI 可见）。
//!
//! 回显只说「已重载 + 生效边界 + 文件数」，NEVER 做字段级 diff
//! （配置字段过多，逐项列举既嘈杂又难维护）。

use crate::config::domain::reload::ComponentReloadSource;
use crate::config::domain::scope::ConfigApplicationScope;

/// config 热载回显：`[config] 已重载。下次 Run 生效。`
///
/// `scopes` 为空时只报已重载，不臆测生效边界。
pub fn config_reload_notice(lang: &str, scopes: &[ConfigApplicationScope]) -> String {
    let effects = scope_effects(lang, scopes);
    match lang {
        "zh" => {
            if effects.is_empty() {
                "[config] 已重载。".to_string()
            } else {
                format!("[config] 已重载。{}。", effects.join("；"))
            }
        }
        _ => {
            if effects.is_empty() {
                "[config] reloaded.".to_string()
            } else {
                format!("[config] reloaded. {}.", effects.join("; "))
            }
        }
    }
}

/// 组件热载回显：`[guidance] 已重载（2 个文件）。`
pub fn component_reload_notice(
    lang: &str,
    source: ComponentReloadSource,
    file_count: usize,
) -> String {
    let label = source.label();
    match lang {
        "zh" => format!("[{label}] 已重载（{file_count} 个文件）。"),
        _ => format!("[{label}] reloaded ({file_count} file(s))."),
    }
}

fn scope_effects(lang: &str, scopes: &[ConfigApplicationScope]) -> Vec<&'static str> {
    let mut effects = Vec::new();
    if scopes.contains(&ConfigApplicationScope::Immediate) {
        effects.push(match lang {
            "zh" => "立即生效",
            _ => "effective immediately",
        });
    }
    if scopes.contains(&ConfigApplicationScope::Run) {
        effects.push(match lang {
            "zh" => "下次 Run 生效",
            _ => "effective next run",
        });
    }
    if scopes.contains(&ConfigApplicationScope::SessionRestartRequired) {
        effects.push(match lang {
            "zh" => "重启 Session 后生效",
            _ => "effective after session restart",
        });
    }
    effects
}

#[cfg(test)]
#[path = "config_reload_tests.rs"]
mod tests;
