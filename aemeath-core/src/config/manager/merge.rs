//! Config merge logic

use super::*;

impl ConfigManager {
    /// Merge two configs (overlay takes precedence)
    pub(crate) fn merge_config(base: Config, overlay: Config) -> Config {
        Config {
            api: ApiConfig {
                // None = unset, use base value; Some = explicitly set
                provider: overlay.api.provider.or(base.api.provider),
                key: overlay.api.key.or(base.api.key),
                base_url: overlay.api.base_url.or(base.api.base_url),
                user_agent: if overlay.api.user_agent != legacy::default_user_agent() {
                    overlay.api.user_agent
                } else {
                    base.api.user_agent
                },
                timeout: if overlay.api.timeout != legacy::default_timeout() {
                    overlay.api.timeout
                } else {
                    base.api.timeout
                },
                retries: if overlay.api.retries != legacy::default_retries() {
                    overlay.api.retries
                } else {
                    base.api.retries
                },
            },
            model: ModelConfig {
                name: if overlay.model.name != legacy::default_model() {
                    overlay.model.name
                } else {
                    base.model.name
                },
                max_tokens: if overlay.model.max_tokens != legacy::default_max_tokens() {
                    overlay.model.max_tokens
                } else {
                    base.model.max_tokens
                },
                context_size: if overlay.model.context_size != legacy::default_context_size() {
                    overlay.model.context_size
                } else {
                    base.model.context_size
                },
                temperature: overlay.model.temperature.or(base.model.temperature),
                top_k: overlay.model.top_k.or(base.model.top_k),
                top_p: overlay.model.top_p.or(base.model.top_p),
                stop_sequences: if !overlay.model.stop_sequences.is_empty() {
                    overlay.model.stop_sequences
                } else {
                    base.model.stop_sequences
                },
            },
            models: {
                // Merge sources from both configs (JSON field remains models.providers)
                let mut providers = base.models.providers;
                for (k, v) in overlay.models.providers {
                    providers.insert(k, v);
                }
                // Merge guidance from both configs
                let mut guidance = base.models.guidance;
                for (k, v) in overlay.models.guidance {
                    guidance.insert(k, v);
                }
                ModelsConfig {
                    mode: if overlay.models.mode.is_empty() {
                        base.models.mode
                    } else {
                        overlay.models.mode
                    },
                    default: if overlay.models.default.is_empty() {
                        base.models.default
                    } else {
                        overlay.models.default
                    },
                    providers,
                    guidance,
                }
            },
            tools: ToolsConfig {
                enabled: if !overlay.tools.enabled.is_empty() {
                    overlay.tools.enabled
                } else {
                    base.tools.enabled
                },
                disabled: if !overlay.tools.disabled.is_empty() {
                    overlay.tools.disabled
                } else {
                    base.tools.disabled
                },
                settings: Self::merge_maps(base.tools.settings, overlay.tools.settings),
                max_concurrency: if overlay.tools.max_concurrency
                    != tools::default_max_tool_concurrency()
                {
                    overlay.tools.max_concurrency
                } else {
                    base.tools.max_concurrency
                },
            },
            agents: AgentsConfig {
                max_concurrency: if overlay.agents.max_concurrency
                    != tools::default_max_agent_concurrency()
                {
                    overlay.agents.max_concurrency
                } else {
                    base.agents.max_concurrency
                },
                roles: {
                    let mut roles = base.agents.roles;
                    for (k, v) in overlay.agents.roles {
                        roles.insert(k, v);
                    }
                    roles
                },
                default_model: if !overlay.agents.default_model.is_empty() {
                    overlay.agents.default_model
                } else {
                    base.agents.default_model
                },
            },
            ui: UiConfig {
                markdown: overlay.ui.markdown,
                syntax_highlight: overlay.ui.syntax_highlight,
                progress: overlay.ui.progress,
                color: overlay.ui.color,
                verbose: overlay.ui.verbose || base.ui.verbose,
                tui: overlay.ui.tui,
            },
            permissions: PermissionConfig {
                mode: if overlay.permissions.mode != PermissionModeConfig::default() {
                    overlay.permissions.mode
                } else {
                    base.permissions.mode
                },
                auto_approve: if !overlay.permissions.auto_approve.is_empty() {
                    overlay.permissions.auto_approve
                } else {
                    base.permissions.auto_approve
                },
                deny: if !overlay.permissions.deny.is_empty() {
                    overlay.permissions.deny
                } else {
                    base.permissions.deny
                },
            },
            storage: StorageConfig {
                sessions_dir: overlay.storage.sessions_dir.or(base.storage.sessions_dir),
                // For boolean/numeric fields we cannot distinguish "unset" from "set to default"
                // using serde defaults. Use overlay directly — the user chose these values.
                persist_sessions: overlay.storage.persist_sessions,
                max_sessions: overlay.storage.max_sessions,
                history: overlay.storage.history,
                history_file: overlay.storage.history_file.or(base.storage.history_file),
            },
            skills: SkillsConfig {
                dirs: if !overlay.skills.dirs.is_empty() {
                    overlay.skills.dirs
                } else {
                    base.skills.dirs
                },
            },
            hooks: {
                // Merge hooks: overlay takes precedence for each event type
                let mut events = base.hooks.events;
                for (k, v) in overlay.hooks.events {
                    events.insert(k, v);
                }
                HooksConfig { events }
            },
            memory: overlay.memory,
        }
    }

    /// Merge two hashmaps
    pub(crate) fn merge_maps(
        base: std::collections::HashMap<String, serde_json::Value>,
        overlay: std::collections::HashMap<String, serde_json::Value>,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut result = base;
        result.extend(overlay);
        result
    }
}
