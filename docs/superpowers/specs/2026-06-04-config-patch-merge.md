# Config Patch Merge Spec

## 背景

当前配置文件直接反序列化为完整 `Config`。因为 `Config` 及其子配置带有 serde/default，配置文件缺失字段会被填成硬编码默认值，后续 `merge_config(base, overlay)` 无法区分：

- 字段缺失：不应该覆盖低优先级配置。
- 显式配置为默认值：应该覆盖低优先级配置。

已确认的问题：全局 `~/.agents/aemeath.json` 设置 `logging.level = "debug"`，项目 `.agents/aemeath.json` 只写 hooks 时，项目配置缺失的 `logging.level` 被反序列化为默认 `"warn"`，最终覆盖全局 `debug`，导致 info hook 日志默认不可见。

同类风险存在于所有 bool 字段、默认非空 string、默认非零 number、默认 enum、默认 struct 字段。

## 目标

1. 配置文件读取 MUST 保留字段缺失语义。
2. 高优先级配置文件缺失字段 MUST NOT 覆盖低优先级配置。
3. 高优先级配置文件显式配置字段 MUST 覆盖低优先级配置，即使值等于硬编码默认值。
4. 最终运行时 `Config` MUST 仍是完整配置，调用方不需要处理 `Option`。
5. 全部现有配置优先级 MUST 保持不变：默认值 < 全局配置 < Claude project settings < 项目配置 < 环境变量 < CLI 参数。
6. hooks 的事件 map 合并语义 MUST 保持：高优先级事件 key 覆盖同名低优先级事件，未提及事件保留。
7. map 型配置（如 models providers/guidance、tools settings、agents roles）MUST 保持逐 key 合并语义。
8. list 型配置（如 tools enabled/disabled、skills dirs、permission auto_approve/deny、model stop_sequences）MUST 保持“显式提供即整体覆盖”的语义。
9. 本次 MUST 覆盖所有当前 bool 字段，避免 false/true 与缺失混淆。

## 设计

新增只用于配置文件输入的 Patch DTO：

- `ConfigPatch`
- `ApiConfigPatch`
- `ModelConfigPatch`
- `ModelsConfigPatch`
- `ToolsConfigPatch`
- `AgentsConfigPatch`
- `UiConfigPatch`
- `TaskListConfigPatch`
- `TaskLifecycleConfigPatch`
- `PermissionConfigPatch`
- `SkillsConfigPatch`
- `StorageConfigPatch`
- `MemoryConfigPatch`
- `ReflectionConfigPatch`
- `LoggingConfigPatch`
- `SubAgentLogConfigPatch`

Patch DTO 字段用 `Option<T>` 表示是否显式出现。例如：

- 缺失 `logging.level` => `None`，不覆盖。
- 显式 `logging.level = "warn"` => `Some("warn")`，覆盖。
- 缺失 `ui.color` => `None`，不覆盖。
- 显式 `ui.color = false` => `Some(false)`，覆盖。

`Config` 保持最终运行时 DTO，不改使用方。

## 合并规则

新增 `ConfigManager::apply_patch(base: Config, patch: ConfigPatch) -> Config`。

### 标量 Option 字段

`patch.field.unwrap_or(base.field)`。

### 嵌套 struct

只有 patch 中对应子对象存在时，调用子对象 patch apply；缺失时保留 base 子对象。

### map 字段

显式提供 map 时逐 key extend 到 base map。

### list 字段

显式提供 list 时整体替换 base list；缺失时保留 base list。

### hooks

由于 `HooksConfig` 已是 map 型结构，可在 patch 中使用 `Option<HooksConfig>`；存在时按现有事件 key 合并。

### Claude settings

Claude Code project settings 当前只转换 hooks。它没有字段缺失默认覆盖风险，可以继续转换为完整 `Config` 后使用旧合并，或转换为 `ConfigPatch { hooks: Some(...), ..Default::default() }`。推荐后者，以统一配置文件合并入口。

## 需覆盖的 bool 字段

- `ui.markdown`
- `ui.syntax_highlight`
- `ui.progress`
- `ui.color`
- `ui.verbose`
- `ui.tui`
- `ui.task_lifecycle.auto_clear_completed_on_new_turn`
- `ui.task_lifecycle.interrupt_prompt_enabled`
- `memory.enabled`
- `memory.auto_summary_on_session_end`
- `memory.reflection.enabled`
- `memory.reflection.auto_apply_suggestions`
- `logging.sub_agent_log.enabled`
- `logging.sub_agent_log.include_request_payload`
- `logging.role_logs_enabled`
- `storage.persist_sessions`
- `storage.history`

## 测试要求

1. 全局 `logging.level = debug`，项目配置只写 hooks，最终必须保持 `debug`。
2. 全局 `logging.level = debug`，项目显式 `logging.level = warn`，最终必须是 `warn`。
3. 全局 bool 为 `false`，项目缺失该字段时最终保持 `false`。
4. 全局 bool 为 `true`，项目显式 `false` 时最终为 `false`。
5. 默认 false 的 bool（`ui.verbose`、`memory.reflection.auto_apply_suggestions`）必须支持全局 true + 项目缺失 => true，以及项目显式 false => false。
6. hooks、models providers/guidance、tools settings、agents roles 仍逐 key 合并。
7. tools enabled/disabled、skills dirs、permissions lists、model stop_sequences 显式提供时整体覆盖，缺失时保留。
8. 环境变量仍可覆盖 patch 合并后的最终配置。

## 非目标

1. 不改变最终 `Config` 结构。
2. 不改变保存配置 `save_global` / `save_project` 的完整配置输出格式。
3. 不改变 provider/model 业务默认值。
4. 不改变配置文件路径优先级。
