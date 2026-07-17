use share::config::hooks::{default_timeout_secs, ClaudeSettingsConfig, HookEntry};
use share::config::HooksConfig;
use std::collections::HashMap;

pub trait ClaudeSettingsAdapter {
    fn into_hooks_config(self) -> HooksConfig;
}

impl ClaudeSettingsAdapter for ClaudeSettingsConfig {
    fn into_hooks_config(self) -> HooksConfig {
        let mut events = HashMap::new();
        for (event, groups) in self.hooks {
            let mut entries = Vec::new();
            for group in groups {
                for hook in group.hooks {
                    if hook.command.trim().is_empty() {
                        continue;
                    }
                    entries.push(HookEntry {
                        matcher: group.matcher.clone(),
                        command: hook.command,
                        timeout: hook.timeout.unwrap_or_else(default_timeout_secs),
                    });
                }
            }
            if !entries.is_empty() {
                events.insert(event, entries);
            }
        }
        HooksConfig { events }
    }
}

#[cfg(test)]
#[path = "claude_settings_adapter_tests.rs"]
mod tests;
