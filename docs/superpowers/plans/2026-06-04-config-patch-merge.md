# Config Patch Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 optional/patch config 输入 DTO，修复配置文件缺失字段覆盖低优先级显式配置的问题，覆盖所有 bool 字段与现有合并语义。

**Architecture:** 保持 `share::config::Config` 作为最终运行时完整配置；在 runtime config manager 内新增 `ConfigPatch` 及子 patch DTO 作为配置文件反序列化入口。加载全局/项目配置时从 JSON 反序列化为 patch，再把 patch 应用到 base `Config`，因此字段缺失不会覆盖，显式默认值仍会覆盖。

**Tech Stack:** Rust、serde、tokio、cargo test。

---

## File Structure

- Modify: `agent/features/runtime/src/utils/bootstrap/config_manager.rs`
  - 新增 patch DTO。
  - 新增 `apply_patch` 及子 apply helpers。
  - 修改 `load()` 使用 `ConfigPatch` 读取全局/项目配置。
  - 将 Claude settings 转为 hooks patch。
  - 增加配置合并回归测试。
- Create: `docs/superpowers/specs/2026-06-04-config-patch-merge.md`
  - 记录设计与范围。
- Create: `docs/superpowers/plans/2026-06-04-config-patch-merge.md`
  - 本实施计划。

## Task 1: Failing Tests for Logging and Bool Patch Semantics

**Files:**
- Modify: `agent/features/runtime/src/utils/bootstrap/config_manager.rs`

- [ ] **Step 1: Add tests proving missing project logging does not override global debug**

在 `config_manager.rs` 文件末尾 `#[cfg(test)] mod tests` 中新增测试。如果 tests 模块尚不存在，则新建：

```rust
#[test]
fn test_apply_patch_project_hooks_do_not_reset_global_logging_level() {
    let base = Config {
        logging: LoggingConfig {
            level: "debug".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let patch: ConfigPatch = serde_json::from_str(
        r#"{
          "hooks": {
            "Stop": [{ "command": "echo ok" }]
          }
        }"#,
    )
    .expect("project hooks patch should parse");

    let merged = ConfigManager::apply_patch(base, patch);

    assert_eq!(merged.logging.level, "debug");
    assert_eq!(merged.hooks.events.len(), 1);
}

#[test]
fn test_apply_patch_project_can_explicitly_override_logging_level_to_warn() {
    let base = Config {
        logging: LoggingConfig {
            level: "debug".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let patch: ConfigPatch = serde_json::from_str(
        r#"{
          "logging": { "level": "warn" }
        }"#,
    )
    .expect("logging patch should parse");

    let merged = ConfigManager::apply_patch(base, patch);

    assert_eq!(merged.logging.level, "warn");
}
```

- [ ] **Step 2: Add tests covering all bool families**

新增测试：

```rust
#[test]
fn test_apply_patch_missing_bool_fields_preserve_lower_priority_values() {
    let base = Config {
        ui: UiConfig {
            markdown: false,
            syntax_highlight: false,
            progress: false,
            color: false,
            verbose: true,
            tui: false,
            task_lifecycle: TaskLifecycleConfig {
                auto_clear_completed_on_new_turn: false,
                interrupt_prompt_enabled: false,
                ..Default::default()
            },
            ..Default::default()
        },
        storage: StorageConfig {
            persist_sessions: false,
            history: false,
            ..Default::default()
        },
        memory: MemoryConfig {
            enabled: false,
            auto_summary_on_session_end: false,
            reflection: ReflectionConfig {
                enabled: false,
                auto_apply_suggestions: true,
                ..Default::default()
            },
            ..Default::default()
        },
        logging: LoggingConfig {
            sub_agent_log: SubAgentLogConfig {
                enabled: false,
                include_request_payload: false,
                ..Default::default()
            },
            role_logs_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let patch: ConfigPatch = serde_json::from_str(r#"{}"#).expect("empty patch should parse");

    let merged = ConfigManager::apply_patch(base, patch);

    assert!(!merged.ui.markdown);
    assert!(!merged.ui.syntax_highlight);
    assert!(!merged.ui.progress);
    assert!(!merged.ui.color);
    assert!(merged.ui.verbose);
    assert!(!merged.ui.tui);
    assert!(!merged.ui.task_lifecycle.auto_clear_completed_on_new_turn);
    assert!(!merged.ui.task_lifecycle.interrupt_prompt_enabled);
    assert!(!merged.storage.persist_sessions);
    assert!(!merged.storage.history);
    assert!(!merged.memory.enabled);
    assert!(!merged.memory.auto_summary_on_session_end);
    assert!(!merged.memory.reflection.enabled);
    assert!(merged.memory.reflection.auto_apply_suggestions);
    assert!(!merged.logging.sub_agent_log.enabled);
    assert!(!merged.logging.sub_agent_log.include_request_payload);
    assert!(!merged.logging.role_logs_enabled);
}

#[test]
fn test_apply_patch_explicit_bool_fields_override_lower_priority_values() {
    let base = Config {
        ui: UiConfig {
            markdown: true,
            syntax_highlight: true,
            progress: true,
            color: true,
            verbose: true,
            tui: true,
            task_lifecycle: TaskLifecycleConfig {
                auto_clear_completed_on_new_turn: true,
                interrupt_prompt_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        },
        storage: StorageConfig {
            persist_sessions: true,
            history: true,
            ..Default::default()
        },
        memory: MemoryConfig {
            enabled: true,
            auto_summary_on_session_end: true,
            reflection: ReflectionConfig {
                enabled: true,
                auto_apply_suggestions: true,
                ..Default::default()
            },
            ..Default::default()
        },
        logging: LoggingConfig {
            sub_agent_log: SubAgentLogConfig {
                enabled: true,
                include_request_payload: true,
                ..Default::default()
            },
            role_logs_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let patch: ConfigPatch = serde_json::from_str(
        r#"{
          "ui": {
            "markdown": false,
            "syntax_highlight": false,
            "progress": false,
            "color": false,
            "verbose": false,
            "tui": false,
            "task_lifecycle": {
              "auto_clear_completed_on_new_turn": false,
              "interrupt_prompt_enabled": false
            }
          },
          "storage": {
            "persist_sessions": false,
            "history": false
          },
          "memory": {
            "enabled": false,
            "auto_summary_on_session_end": false,
            "reflection": {
              "enabled": false,
              "auto_apply_suggestions": false
            }
          },
          "logging": {
            "sub_agent_log": {
              "enabled": false,
              "include_request_payload": false
            },
            "role_logs_enabled": false
          }
        }"#,
    )
    .expect("bool patch should parse");

    let merged = ConfigManager::apply_patch(base, patch);

    assert!(!merged.ui.markdown);
    assert!(!merged.ui.syntax_highlight);
    assert!(!merged.ui.progress);
    assert!(!merged.ui.color);
    assert!(!merged.ui.verbose);
    assert!(!merged.ui.tui);
    assert!(!merged.ui.task_lifecycle.auto_clear_completed_on_new_turn);
    assert!(!merged.ui.task_lifecycle.interrupt_prompt_enabled);
    assert!(!merged.storage.persist_sessions);
    assert!(!merged.storage.history);
    assert!(!merged.memory.enabled);
    assert!(!merged.memory.auto_summary_on_session_end);
    assert!(!merged.memory.reflection.enabled);
    assert!(!merged.memory.reflection.auto_apply_suggestions);
    assert!(!merged.logging.sub_agent_log.enabled);
    assert!(!merged.logging.sub_agent_log.include_request_payload);
    assert!(!merged.logging.role_logs_enabled);
}
```

- [ ] **Step 3: Run targeted tests and verify failure**

Run:

```bash
cargo test -p runtime apply_patch -- --nocapture
```

Expected: FAIL because `ConfigPatch` and `apply_patch` do not exist yet.

## Task 2: Add Patch DTOs

**Files:**
- Modify: `agent/features/runtime/src/utils/bootstrap/config_manager.rs`

- [ ] **Step 1: Add imports for HashMap and Value**

At top of file, extend imports:

```rust
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, path::{Path, PathBuf}};
```

- [ ] **Step 2: Add patch structs after default helper functions**

Add complete patch DTO definitions after `default_sub_agent_log_config()`:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ConfigPatch {
    #[serde(default)]
    api: Option<ApiConfigPatch>,
    #[serde(default)]
    model: Option<ModelConfigPatch>,
    #[serde(default)]
    models: Option<ModelsConfigPatch>,
    #[serde(default)]
    tools: Option<ToolsConfigPatch>,
    #[serde(default)]
    agents: Option<AgentsConfigPatch>,
    #[serde(default)]
    ui: Option<UiConfigPatch>,
    #[serde(default)]
    permissions: Option<PermissionConfigPatch>,
    #[serde(default)]
    skills: Option<SkillsConfigPatch>,
    #[serde(default)]
    storage: Option<StorageConfigPatch>,
    #[serde(default)]
    hooks: Option<HooksConfig>,
    #[serde(default)]
    memory: Option<MemoryConfigPatch>,
    #[serde(default)]
    logging: Option<LoggingConfigPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ApiConfigPatch {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    retries: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ModelConfigPatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    context_size: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_k: Option<u32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ModelsConfigPatch {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    providers: Option<HashMap<String, share::config::models::ProviderModelsConfig>>,
    #[serde(default)]
    guidance: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ToolsConfigPatch {
    #[serde(default)]
    enabled: Option<Vec<String>>,
    #[serde(default)]
    disabled: Option<Vec<String>>,
    #[serde(default)]
    settings: Option<HashMap<String, Value>>,
    #[serde(default)]
    max_concurrency: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AgentsConfigPatch {
    #[serde(default)]
    max_concurrency: Option<usize>,
    #[serde(default)]
    roles: Option<HashMap<String, share::config::tools::AgentRoleConfig>>,
    #[serde(default)]
    default_model: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct UiConfigPatch {
    #[serde(default)]
    markdown: Option<bool>,
    #[serde(default)]
    syntax_highlight: Option<bool>,
    #[serde(default)]
    progress: Option<bool>,
    #[serde(default)]
    color: Option<bool>,
    #[serde(default)]
    verbose: Option<bool>,
    #[serde(default)]
    tui: Option<bool>,
    #[serde(default)]
    task_list: Option<TaskListConfigPatch>,
    #[serde(default)]
    task_lifecycle: Option<TaskLifecycleConfigPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TaskListConfigPatch {
    #[serde(default)]
    max_lines: Option<usize>,
    #[serde(default)]
    fold_hint_format: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TaskLifecycleConfigPatch {
    #[serde(default)]
    auto_clear_completed_on_new_turn: Option<bool>,
    #[serde(default)]
    interrupt_prompt_enabled: Option<bool>,
    #[serde(default)]
    interrupt_default_action: Option<String>,
    #[serde(default)]
    stale_remind_after_turns: Option<usize>,
    #[serde(default)]
    stale_remind_repeat_interval: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PermissionConfigPatch {
    #[serde(default)]
    mode: Option<PermissionModeConfig>,
    #[serde(default)]
    auto_approve: Option<Vec<String>>,
    #[serde(default)]
    deny: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SkillsConfigPatch {
    #[serde(default)]
    dirs: Option<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct StorageConfigPatch {
    #[serde(default)]
    sessions_dir: Option<PathBuf>,
    #[serde(default)]
    persist_sessions: Option<bool>,
    #[serde(default)]
    max_sessions: Option<usize>,
    #[serde(default)]
    history: Option<bool>,
    #[serde(default)]
    history_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MemoryConfigPatch {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    max_entries: Option<usize>,
    #[serde(default)]
    max_inject_count: Option<usize>,
    #[serde(default)]
    auto_summary_on_session_end: Option<bool>,
    #[serde(default)]
    similarity_threshold: Option<f64>,
    #[serde(default)]
    reflection: Option<ReflectionConfigPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ReflectionConfigPatch {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    interval_turns: Option<usize>,
    #[serde(default)]
    auto_apply_suggestions: Option<bool>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct LoggingConfigPatch {
    #[serde(default, alias = "default_level")]
    level: Option<String>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    max_backups: Option<usize>,
    #[serde(default)]
    retention_days: Option<u64>,
    #[serde(default)]
    sub_agent_log: Option<SubAgentLogConfigPatch>,
    #[serde(default)]
    logs_dir: Option<String>,
    #[serde(default)]
    role_logs_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SubAgentLogConfigPatch {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    include_request_payload: Option<bool>,
    #[serde(default)]
    max_payload_bytes: Option<usize>,
}
```

## Task 3: Implement apply_patch Helpers

**Files:**
- Modify: `agent/features/runtime/src/utils/bootstrap/config_manager.rs`

- [ ] **Step 1: Add `apply_patch` in `impl ConfigManager` before `merge_config`**

Add:

```rust
pub(crate) fn apply_patch(mut base: Config, patch: ConfigPatch) -> Config {
    if let Some(api) = patch.api {
        base.api = Self::apply_api_patch(base.api, api);
    }
    if let Some(model) = patch.model {
        base.model = Self::apply_model_patch(base.model, model);
    }
    if let Some(models) = patch.models {
        base.models = Self::apply_models_patch(base.models, models);
    }
    if let Some(tools) = patch.tools {
        base.tools = Self::apply_tools_patch(base.tools, tools);
    }
    if let Some(agents) = patch.agents {
        base.agents = Self::apply_agents_patch(base.agents, agents);
    }
    if let Some(ui) = patch.ui {
        base.ui = Self::apply_ui_patch(base.ui, ui);
    }
    if let Some(permissions) = patch.permissions {
        base.permissions = Self::apply_permission_patch(base.permissions, permissions);
    }
    if let Some(skills) = patch.skills {
        base.skills = Self::apply_skills_patch(base.skills, skills);
    }
    if let Some(storage) = patch.storage {
        base.storage = Self::apply_storage_patch(base.storage, storage);
    }
    if let Some(hooks) = patch.hooks {
        base.hooks = Self::merge_hooks(base.hooks, hooks);
    }
    if let Some(memory) = patch.memory {
        base.memory = Self::apply_memory_patch(base.memory, memory);
    }
    if let Some(logging) = patch.logging {
        base.logging = Self::apply_logging_patch(base.logging, logging);
    }
    base
}
```

- [ ] **Step 2: Add helper functions**

Add these helper functions in `impl ConfigManager`:

```rust
fn apply_api_patch(mut base: ApiConfig, patch: ApiConfigPatch) -> ApiConfig {
    if let Some(v) = patch.provider { base.provider = Some(v); }
    if let Some(v) = patch.key { base.key = Some(v); }
    if let Some(v) = patch.base_url { base.base_url = Some(v); }
    if let Some(v) = patch.user_agent { base.user_agent = v; }
    if let Some(v) = patch.timeout { base.timeout = v; }
    if let Some(v) = patch.retries { base.retries = v; }
    base
}

fn apply_model_patch(mut base: ModelConfig, patch: ModelConfigPatch) -> ModelConfig {
    if let Some(v) = patch.name { base.name = v; }
    if let Some(v) = patch.max_tokens { base.max_tokens = v; }
    if let Some(v) = patch.context_size { base.context_size = v; }
    if let Some(v) = patch.temperature { base.temperature = Some(v); }
    if let Some(v) = patch.top_k { base.top_k = Some(v); }
    if let Some(v) = patch.top_p { base.top_p = Some(v); }
    if let Some(v) = patch.stop_sequences { base.stop_sequences = v; }
    base
}

fn apply_models_patch(mut base: ModelsConfig, patch: ModelsConfigPatch) -> ModelsConfig {
    if let Some(v) = patch.mode { base.mode = v; }
    if let Some(v) = patch.default { base.default = v; }
    if let Some(providers) = patch.providers {
        for (k, v) in providers { base.providers.insert(k, v); }
    }
    if let Some(guidance) = patch.guidance {
        for (k, v) in guidance { base.guidance.insert(k, v); }
    }
    base
}

fn apply_tools_patch(mut base: ToolsConfig, patch: ToolsConfigPatch) -> ToolsConfig {
    if let Some(v) = patch.enabled { base.enabled = v; }
    if let Some(v) = patch.disabled { base.disabled = v; }
    if let Some(settings) = patch.settings {
        for (k, v) in settings { base.settings.insert(k, v); }
    }
    if let Some(v) = patch.max_concurrency { base.max_concurrency = v; }
    base
}

fn apply_agents_patch(mut base: AgentsConfig, patch: AgentsConfigPatch) -> AgentsConfig {
    if let Some(v) = patch.max_concurrency { base.max_concurrency = v; }
    if let Some(roles) = patch.roles {
        for (k, v) in roles { base.roles.insert(k, v); }
    }
    if let Some(v) = patch.default_model { base.default_model = v; }
    base
}

fn apply_ui_patch(mut base: UiConfig, patch: UiConfigPatch) -> UiConfig {
    if let Some(v) = patch.markdown { base.markdown = v; }
    if let Some(v) = patch.syntax_highlight { base.syntax_highlight = v; }
    if let Some(v) = patch.progress { base.progress = v; }
    if let Some(v) = patch.color { base.color = v; }
    if let Some(v) = patch.verbose { base.verbose = v; }
    if let Some(v) = patch.tui { base.tui = v; }
    if let Some(v) = patch.task_list { base.task_list = Self::apply_task_list_patch(base.task_list, v); }
    if let Some(v) = patch.task_lifecycle { base.task_lifecycle = Self::apply_task_lifecycle_patch(base.task_lifecycle, v); }
    base
}

fn apply_task_list_patch(mut base: TaskListConfig, patch: TaskListConfigPatch) -> TaskListConfig {
    if let Some(v) = patch.max_lines { base.max_lines = v; }
    if let Some(v) = patch.fold_hint_format { base.fold_hint_format = v; }
    base
}

fn apply_task_lifecycle_patch(mut base: TaskLifecycleConfig, patch: TaskLifecycleConfigPatch) -> TaskLifecycleConfig {
    if let Some(v) = patch.auto_clear_completed_on_new_turn { base.auto_clear_completed_on_new_turn = v; }
    if let Some(v) = patch.interrupt_prompt_enabled { base.interrupt_prompt_enabled = v; }
    if let Some(v) = patch.interrupt_default_action { base.interrupt_default_action = v; }
    if let Some(v) = patch.stale_remind_after_turns { base.stale_remind_after_turns = v; }
    if let Some(v) = patch.stale_remind_repeat_interval { base.stale_remind_repeat_interval = v; }
    base
}

fn apply_permission_patch(mut base: PermissionConfig, patch: PermissionConfigPatch) -> PermissionConfig {
    if let Some(v) = patch.mode { base.mode = v; }
    if let Some(v) = patch.auto_approve { base.auto_approve = v; }
    if let Some(v) = patch.deny { base.deny = v; }
    base
}

fn apply_skills_patch(mut base: SkillsConfig, patch: SkillsConfigPatch) -> SkillsConfig {
    if let Some(v) = patch.dirs { base.dirs = v; }
    base
}

fn apply_storage_patch(mut base: StorageConfig, patch: StorageConfigPatch) -> StorageConfig {
    if let Some(v) = patch.sessions_dir { base.sessions_dir = Some(v); }
    if let Some(v) = patch.persist_sessions { base.persist_sessions = v; }
    if let Some(v) = patch.max_sessions { base.max_sessions = v; }
    if let Some(v) = patch.history { base.history = v; }
    if let Some(v) = patch.history_file { base.history_file = Some(v); }
    base
}

fn merge_hooks(base: HooksConfig, overlay: HooksConfig) -> HooksConfig {
    let mut events = base.events;
    for (k, v) in overlay.events { events.insert(k, v); }
    HooksConfig { events }
}

fn apply_memory_patch(mut base: MemoryConfig, patch: MemoryConfigPatch) -> MemoryConfig {
    if let Some(v) = patch.enabled { base.enabled = v; }
    if let Some(v) = patch.max_entries { base.max_entries = v; }
    if let Some(v) = patch.max_inject_count { base.max_inject_count = v; }
    if let Some(v) = patch.auto_summary_on_session_end { base.auto_summary_on_session_end = v; }
    if let Some(v) = patch.similarity_threshold { base.similarity_threshold = v; }
    if let Some(v) = patch.reflection { base.reflection = Self::apply_reflection_patch(base.reflection, v); }
    base
}

fn apply_reflection_patch(mut base: ReflectionConfig, patch: ReflectionConfigPatch) -> ReflectionConfig {
    if let Some(v) = patch.enabled { base.enabled = v; }
    if let Some(v) = patch.interval_turns { base.interval_turns = v; }
    if let Some(v) = patch.auto_apply_suggestions { base.auto_apply_suggestions = v; }
    if let Some(v) = patch.model { base.model = Some(v); }
    base
}

fn apply_logging_patch(mut base: LoggingConfig, patch: LoggingConfigPatch) -> LoggingConfig {
    if let Some(v) = patch.level { base.level = v; }
    if let Some(v) = patch.max_bytes { base.max_bytes = v; }
    if let Some(v) = patch.max_backups { base.max_backups = v; }
    if let Some(v) = patch.retention_days { base.retention_days = v; }
    if let Some(v) = patch.sub_agent_log { base.sub_agent_log = Self::apply_sub_agent_log_patch(base.sub_agent_log, v); }
    if let Some(v) = patch.logs_dir { base.logs_dir = Some(v); }
    if let Some(v) = patch.role_logs_enabled { base.role_logs_enabled = v; }
    base
}

fn apply_sub_agent_log_patch(mut base: SubAgentLogConfig, patch: SubAgentLogConfigPatch) -> SubAgentLogConfig {
    if let Some(v) = patch.enabled { base.enabled = v; }
    if let Some(v) = patch.include_request_payload { base.include_request_payload = v; }
    if let Some(v) = patch.max_payload_bytes { base.max_payload_bytes = v; }
    base
}
```

- [ ] **Step 3: Run targeted tests**

Run:

```bash
cargo test -p runtime apply_patch -- --nocapture
```

Expected: PASS.

## Task 4: Wire File Loading to ConfigPatch

**Files:**
- Modify: `agent/features/runtime/src/utils/bootstrap/config_manager.rs`

- [ ] **Step 1: Replace global config deserialization**

Change:

```rust
serde_json::from_str::<Config>(&content)
```

to:

```rust
serde_json::from_str::<ConfigPatch>(&content)
```

and change variable names from `global_config` to `global_patch`, applying with:

```rust
config = Self::apply_patch(config, global_patch)
```

- [ ] **Step 2: Replace project config deserialization**

Change project `.agents/aemeath.json` deserialization the same way:

```rust
serde_json::from_str::<ConfigPatch>(&content)
```

and apply with:

```rust
config = Self::apply_patch(config, project_patch)
```

- [ ] **Step 3: Convert Claude settings to hooks patch**

Change:

```rust
config = Self::merge_config(config, claude_config.into_config())
```

To:

```rust
config = Self::apply_patch(
    config,
    ConfigPatch {
        hooks: Some(claude_config.into_config().hooks),
        ..Default::default()
    },
)
```

- [ ] **Step 4: Keep `merge_config` only if still used by tests**

Search:

```bash
rg "merge_config" agent/features/runtime/src/utils/bootstrap/config_manager.rs
```

If only old tests use it, migrate tests to `apply_patch` and remove `merge_config` plus unused default helper functions. If external callers use it, keep it as deprecated internal compatibility.

- [ ] **Step 5: Add integration-style load test with temp dirs**

Add async test:

```rust
#[tokio::test]
async fn test_load_project_hooks_do_not_reset_global_logging_level() {
    let root = tempfile::tempdir().expect("tempdir");
    let home = root.path().join("home_agents");
    let project = root.path().join("project");
    let project_agents = project.join(".agents");
    tokio::fs::create_dir_all(&home).await.unwrap();
    tokio::fs::create_dir_all(&project_agents).await.unwrap();
    tokio::fs::write(
        home.join("aemeath.json"),
        r#"{ "logging": { "level": "debug" } }"#,
    )
    .await
    .unwrap();
    tokio::fs::write(
        project_agents.join("aemeath.json"),
        r#"{ "hooks": { "Stop": [{ "command": "echo ok" }] } }"#,
    )
    .await
    .unwrap();

    let _guard = EnvGuard::set("AEMEATH_AGENTS_DIR", home.to_string_lossy().to_string());
    let manager = ConfigManager::new(Some(&project));

    let loaded = manager.load().await.expect("config should load");

    assert_eq!(loaded.logging.level, "debug");
    assert_eq!(loaded.hooks.events.len(), 1);
}
```

If there is no `EnvGuard`, add a small test-only helper in tests module:

```rust
struct EnvGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: String) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value); }
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(old) = &self.old {
            unsafe { std::env::set_var(self.key, old); }
        } else {
            unsafe { std::env::remove_var(self.key); }
        }
    }
}
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test -p runtime config_manager -- --nocapture
```

Expected: PASS.

## Task 5: Verification and Cleanup

**Files:**
- Modify: `agent/features/runtime/src/utils/bootstrap/config_manager.rs`
- Create: docs files above

- [ ] **Step 1: Run targeted config tests**

```bash
cargo test -p runtime config_manager -- --nocapture
```

Expected: PASS.

- [ ] **Step 2: Run logging bootstrap tests**

```bash
cargo test -p runtime logging -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run format and diff checks**

```bash
cargo fmt -p runtime --check
cargo fmt -p share --check
git diff --check
```

Expected: all PASS / no output from `git diff --check`.

- [ ] **Step 4: Verify real CLI behavior**

Run:

```bash
printf 'hello\n' | cargo run -p cli -- -q -v >/tmp/aemeath-patch-merge.out 2>/tmp/aemeath-patch-merge.err
python3 - <<'PY'
from pathlib import Path
p=Path.home()/'.agents'/'logs'/'aemeath.log'
lines=p.read_text(errors='replace').splitlines()
for line in lines[-300:]:
    if 'logging initialized' in line or 'hook runner built' in line or 'hook match' in line or 'hook start' in line or 'hook end' in line:
        print(line)
PY
```

Expected: without `RUST_LOG`, recent logs include `logging initialized` and hook info lines when global config has `logging.level = debug` and project config only has hooks.

- [ ] **Step 5: Commit**

Use repository commit style and include refs if this is linked to a feature/bug. Suggested message:

```bash
git add agent/features/runtime/src/utils/bootstrap/config_manager.rs docs/superpowers/specs/2026-06-04-config-patch-merge.md docs/superpowers/plans/2026-06-04-config-patch-merge.md
git commit -m "fix(config): 保留配置缺失字段语义"
```

## Self-Review

Spec coverage:
- Patch DTO preserves missing-vs-explicit semantics: Tasks 2-4.
- All bool fields covered: Task 1 tests and Task 3 helpers.
- Final Config remains complete: Task 3 applies patch onto complete base Config.
- Priority order preserved: Task 4 load order unchanged.
- hooks/map/list semantics preserved: Task 3 helpers.
- Env overrides still later: Task 4 does not move `apply_env_vars`.

Placeholder scan:
- No TBD/TODO placeholders.
- Every code-changing step includes concrete code.

Type consistency:
- `ConfigPatch` and all patch structs are defined before use.
- `apply_patch` signature matches tests.
