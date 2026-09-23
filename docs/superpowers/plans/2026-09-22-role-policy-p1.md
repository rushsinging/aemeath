# Role Policy P1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** role 绑定工具策略（白名单 + capability 收缩），取代注册表 `[main, sub]` 静态布尔，成为 sub run 工具裁剪唯一机制。

**Architecture:** config 层新增 `RolePolicyConfig` + 内置 5 role fallback（config 同名整条覆盖）；Tools PL 的 `ToolProfile` 扩展 `allowed_tool_names` 名单维度（`is_authorized` 单点裁剪，`CatalogAdapter::snapshot` 天然过滤 LLM 可见工具）；composition 为每个 role 生成 `role:<name>` profile；runtime `select_tool_catalog` 按 role 选 profile。被裁工具调用走现有 catalog-miss deny（`coordination.rs:471`），Policy 层零改动。

**Tech Stack:** Rust workspace（hexagonal + COLA 分层），设计事实源 `docs/design/02-modules/tools/03-role-policy.md`，门禁 issue #1658（milestone v0.1.0）。

**Worktree:** `.worktrees/design-role-policy`（分支 `design/role-policy`，基于 origin/main）。

**验证基线（每 task 结束运行）：** `cargo test -p <crate>`；Task 8 全量 `cargo fmt --check && cargo test && cargo clippy --workspace -- -D warnings`。

---

### Task 1: Config — RolePolicyConfig + policy 字段 + 内置 5 role + merged_roles

**Files:**
- Modify: `agent/shared/src/config/domain/tools.rs`（`AgentRoleConfig` 120-160 行区、`AgentsConfig` 178-195 行区）
- Modify: `agent/shared/src/config/domain/merge.rs`（`AgentsConfigPatch` 185 行区）
- Test: `agent/shared/src/config/domain/tools_tests.rs`（若无则新建，跟随 crate 现有测试模块模式）

- [ ] **Step 1: 写失败测试**（`tools_tests.rs`）

```rust
#[test]
fn role_policy_parses_allowlist_and_capabilities() {
    let json = r#"{"agents":{"roles":{"searcher":{
        "model":"deepseek/deepseek-chat",
        "policy":{"allowed_tools":["Read","Grep"],"capabilities":["ReadWorkspace"]}}}}}"#;
    let config = serde_json::from_str::<crate::domain::config::ConfigData>(json).unwrap();
    let role = config.agents.roles.get("searcher").unwrap();
    let policy = role.policy.as_ref().unwrap();
    assert_eq!(policy.allowed_tools, vec!["Read", "Grep"]);
    assert_eq!(policy.capabilities, vec!["ReadWorkspace"]);
}

#[test]
fn role_policy_absent_by_default() {
    let json = r#"{"agents":{"roles":{"coder":{"model":"x/y"}}}}"#;
    let config = serde_json::from_str::<crate::domain::config::ConfigData>(json).unwrap();
    assert!(config.agents.roles.get("coder").unwrap().policy.is_none());
}

#[test]
fn merged_roles_config_overrides_builtin_wholesale() {
    let agents = crate::domain::tools::AgentsConfig::default();
    let merged = agents.merged_roles();
    // 内置 5 role 存在
    for name in ["planner", "coder", "searcher", "tester", "reviewer"] {
        assert!(merged.contains_key(name), "missing builtin {name}");
    }
    // config 同名整条覆盖
    let mut agents = crate::domain::tools::AgentsConfig::default();
    agents.roles.insert("coder".into(), crate::domain::tools::AgentRoleConfig {
        model: "qwen/qwen3-coder".into(),
        ..Default::default()
    });
    let merged = agents.merged_roles();
    let coder = merged.get("coder").unwrap();
    assert_eq!(coder.model, "qwen/qwen3-coder");
    assert!(coder.policy.is_none(), "config override must replace builtin policy wholesale");
}
```

- [ ] **Step 2: 运行确认失败**：`cargo test -p aemeath-config role_policy` → 编译错误（无 `policy` 字段 / 无 `merged_roles`）

- [ ] **Step 3: 实现**（`tools.rs`）

```rust
/// Tool policy bound to a role: allowlist + capability restriction.
/// Both fields optional; empty policy means `None` (current behavior).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RolePolicyConfig {
    /// Tool-name allowlist; unlisted tools are invisible to the run.
    #[serde(default, rename = "allowed_tools", alias = "allowedTools")]
    pub allowed_tools: Vec<String>,

    /// Capability-bit restriction, intersected with the allowlist.
    #[serde(default)]
    pub capabilities: Vec<String>,
}
```

`AgentRoleConfig` 增加字段（`max_tokens` 之后）：

```rust
    /// Tool policy for sub runs bound to this role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<RolePolicyConfig>,
```

内置 role 与合并（`AgentsConfig` impl 块，同文件）：

```rust
impl AgentsConfig {
    /// Builtin role fallback; config entries with the same name replace
    /// the builtin definition wholesale (no field-level merge).
    pub fn merged_roles(&self) -> HashMap<String, AgentRoleConfig> {
        let mut merged: HashMap<String, AgentRoleConfig> = builtin_agent_roles()
            .into_iter()
            .map(|(name, role)| (name.to_string(), role))
            .collect();
        for (name, role) in &self.roles {
            merged.insert(name.clone(), role.clone());
        }
        merged
    }
}

fn builtin_agent_roles() -> Vec<(&'static str, AgentRoleConfig)> {
    fn role(policy_tools: &[&str], description: &str) -> AgentRoleConfig {
        AgentRoleConfig {
            enabled: true,
            model: String::new(), // empty → fallback to AgentsConfig::default_model
            description: description.to_string(),
            system_suffix: None,
            max_tokens: None,
            policy: Some(RolePolicyConfig {
                allowed_tools: policy_tools.iter().map(|s| s.to_string()).collect(),
                capabilities: Vec::new(), // derived from tools at compile time
            }),
        }
    }
    vec![
        ("planner", role(&["Read", "Grep", "Glob", "WebSearch", "WebFetch", "TaskGet", "TaskListGet", "TaskLists", "ToolSearch"], "Planning and task breakdown; read-only plus web research")),
        ("coder", role(&["Read", "Write", "Edit", "Glob", "Grep", "Bash", "ToolSearch", "Skill"], "Implementation; read/write/execute, no agent dispatch")),
        ("searcher", role(&["Read", "Grep", "Glob", "WebSearch", "WebFetch", "ToolSearch"], "Local and web code retrieval")),
        ("tester", role(&["Read", "Write", "Edit", "Bash", "Grep", "Glob", "ToolSearch"], "Test authoring and execution")),
        ("reviewer", role(&["Read", "Grep", "Glob", "WebSearch", "ToolSearch"], "Read-only review")),
    ]
}
```

`merge.rs` 的 `AgentsConfigPatch` 对应补 `policy: Option<Option<RolePolicyConfig>>`（跟随该文件现有嵌套 Option merge 模式；若 patch 层对 roles 整体替换则无需改，先读该文件确认再动手）。

- [ ] **Step 4: 运行测试通过**：`cargo test -p aemeath-config`
- [ ] **Step 5: Commit**：`git commit -m "feat(config): AgentRoleConfig 增加 RolePolicyConfig 与内置 role fallback"`

---

### Task 2: Tools PL — ToolProfile 名单维度

**Files:**
- Modify: `agent/features/tools/src/domain/scope_profile.rs`（`ToolProfile` 1-40 行区、`is_authorized` 158-161）
- Modify: `agent/features/tools/src/domain/scope_profile_tests.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn profile_authorizes_only_allowlisted_tool_names() {
    let profile = ToolProfile::baseline_with_names(
        ToolCapabilities::all(),
        ["Read", "Grep"].iter().map(|s| ToolName::new(*s)).collect(),
    );
    let read = ToolRegistrationSpec::new(ToolName::new("Read"), ToolCapabilities::single(ToolCapability::ReadWorkspace));
    let write = ToolRegistrationSpec::new(ToolName::new("Write"), ToolCapabilities::single(ToolCapability::WriteWorkspace));
    assert!(is_authorized(&read, &profile));
    assert!(!is_authorized(&write, &profile));
}

#[test]
fn none_allowlist_keeps_current_authorization() {
    let profile = ToolProfile::baseline(ToolCapabilities::all());
    let write = ToolRegistrationSpec::new(ToolName::new("Write"), ToolCapabilities::single(ToolCapability::WriteWorkspace));
    assert!(is_authorized(&write, &profile));
}

#[test]
fn derive_restricted_rejects_name_expansion() {
    let parent = ToolProfile::baseline_with_names(
        ToolCapabilities::all(),
        ["Read"].iter().map(|s| ToolName::new(*s)).collect(),
    );
    let requested = ["Read", "Write"].iter().map(|s| ToolName::new(*s)).collect();
    let err = ToolProfile::derive_restricted(&parent, ToolCapabilities::all(), Some(requested))
        .expect_err("name expansion must fail");
    assert!(matches!(err, ProfileExpansionError::ToolNameExpansion { .. }));
}
```

- [ ] **Step 2: 运行确认失败**：`cargo test -p aemeath-tools scope_profile`

- [ ] **Step 3: 实现**（`scope_profile.rs`）

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProfile {
    allowed_capabilities: ToolCapabilities,
    allowed_tool_names: Option<BTreeSet<ToolName>>,
}

impl ToolProfile {
    pub fn baseline(allowed_capabilities: ToolCapabilities) -> Self {
        Self { allowed_capabilities, allowed_tool_names: None }
    }

    pub fn baseline_with_names(
        allowed_capabilities: ToolCapabilities,
        allowed_tool_names: BTreeSet<ToolName>,
    ) -> Self {
        Self { allowed_capabilities, allowed_tool_names: Some(allowed_tool_names) }
    }

    pub fn derive_restricted(
        parent: &Self,
        requested: ToolCapabilities,
        requested_names: Option<BTreeSet<ToolName>>,
    ) -> Result<Self, ProfileExpansionError> {
        let expansion = requested & !parent.allowed_capabilities;
        if !expansion.is_empty() {
            return Err(ProfileExpansionError::CapabilityExpansion { capabilities: expansion });
        }
        if let (Some(parent_names), Some(names)) = (&parent.allowed_tool_names, &requested_names) {
            let name_expansion: BTreeSet<_> = names.difference(parent_names).cloned().collect();
            if !name_expansion.is_empty() {
                return Err(ProfileExpansionError::ToolNameExpansion { tools: name_expansion });
            }
        }
        Ok(Self { allowed_capabilities: requested, allowed_tool_names: requested_names })
    }

    pub fn allowed_capabilities(&self) -> ToolCapabilities { self.allowed_capabilities }
    pub fn allowed_tool_names(&self) -> Option<&BTreeSet<ToolName>> { self.allowed_tool_names.as_ref() }
}
```

`ProfileExpansionError` 增加 `ToolNameExpansion { tools: BTreeSet<ToolName> }` 变体（保持 `Copy` 移除，改为 `Clone`，全仓修复使用点）。注意 `ToolProfile` 失去 `Copy`——全仓 `ToolProfile` 传值处补 `.clone()`。

`is_authorized`：

```rust
pub fn is_authorized(spec: &ToolRegistrationSpec, profile: &ToolProfile) -> bool {
    let caps_ok = spec.required_capabilities().is_subset_of(profile.allowed_capabilities);
    let name_ok = profile
        .allowed_tool_names
        .as_ref()
        .map_or(true, |names| names.contains(spec.name()));
    caps_ok && name_ok
}
```

现有 `derive_restricted(parent, requested)` 两参调用点：统一改为 `derive_restricted(parent, requested, None)`（保持现状语义），逐点修复编译。

- [ ] **Step 4: 运行测试通过**：`cargo test -p aemeath-tools`
- [ ] **Step 5: Commit**：`git commit -m "feat(tools): ToolProfile 增加 allowed_tool_names 名单维度与防扩张校验"`

---

### Task 3: Tools PL — RolePolicy 编译 + role profile name

**Files:**
- Create: `agent/features/tools/src/domain/role_policy.rs`
- Modify: `agent/features/tools/src/domain.rs`（加 `pub mod role_policy;`）
- Test: `agent/features/tools/src/domain/role_policy_tests.rs`

- [ ] **Step 1: 写失败测试**

```rust
use crate::domain::role_policy::{compile_role_profile, role_profile_name, RolePolicyCompileError};

fn policy(tools: &[&str], caps: &[&str]) -> share::config::RolePolicyConfig {
    share::config::RolePolicyConfig {
        allowed_tools: tools.iter().map(|s| s.to_string()).collect(),
        capabilities: caps.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn compiles_allowlist_and_derives_capabilities_from_registry() {
    // capability 位由名单内工具的 required capabilities 并集推导（capabilities 字段再做收缩）
    let profile = compile_role_profile(&policy(&["Read", "Grep"], &[]), &|name| {
        (name == "Read").then(|| ToolCapabilities::single(ToolCapability::ReadWorkspace))
            .or_else(|| (name == "Grep").then(|| ToolCapabilities::single(ToolCapability::ReadWorkspace)))
    }).unwrap();
    let names = profile.allowed_tool_names().unwrap();
    assert!(names.contains(&ToolName::new("Read")));
    assert!(!names.contains(&ToolName::new("Write")));
}

#[test]
fn unknown_tool_name_is_rejected() {
    let err = compile_role_profile(&policy(&["NoSuchTool"], &[]), &|_| None).unwrap_err();
    assert!(matches!(err, RolePolicyCompileError::UnknownToolName { .. }));
}

#[test]
fn unknown_capability_is_rejected() {
    let err = compile_role_profile(&policy(&["Read"], &["NotACapability"]), &|_| {
        Some(ToolCapabilities::single(ToolCapability::ReadWorkspace))
    }).unwrap_err();
    assert!(matches!(err, RolePolicyCompileError::UnknownCapability { .. }));
}

#[test]
fn role_profile_name_is_stable_prefix() {
    assert_eq!(role_profile_name("searcher").as_str(), "role:searcher");
}
```

（注：闭包签名以实际注册表查询端口为准——若 `ToolRegistry::get` 需要 `Arc`，编译函数改为接受 `& dyn Fn(&str) -> Option<ToolCapabilities>`，由 composition 传入闭包查 registry。）

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**（`role_policy.rs`）

```rust
//! Compile a config-layer RolePolicyConfig into a ToolProfile.

use std::collections::BTreeSet;

use share::config::RolePolicyConfig;

use super::published_language::{ToolCapabilities, ToolCapability, ToolName, ToolProfileName};
use super::scope_profile::ToolProfile;

#[derive(Debug, thiserror::Error)]
pub enum RolePolicyCompileError {
    #[error("unknown tool name in role policy: {name}")]
    UnknownToolName { name: String },
    #[error("unknown capability in role policy: {name}")]
    UnknownCapability { name: String },
    #[error("role policy allowlist is empty")]
    EmptyAllowlist,
}

/// `role:<name>` profile naming shared by composition (registration) and
/// runtime (lookup); single source of truth.
pub fn role_profile_name(role: &str) -> ToolProfileName {
    ToolProfileName::new(format!("role:{role}"))
}

/// Compile: allowlist → ToolName set (validated), capability bits derived as
/// union of required capabilities of allowlisted tools, then intersected with
/// the declared `capabilities` restriction.
pub fn compile_role_profile(
    policy: &RolePolicyConfig,
    required_caps_of: &dyn Fn(&str) -> Option<ToolCapabilities>,
) -> Result<ToolProfile, RolePolicyCompileError> {
    if policy.allowed_tools.is_empty() {
        return Err(RolePolicyCompileError::EmptyAllowlist);
    }
    let mut names = BTreeSet::new();
    let mut caps = ToolCapabilities::empty();
    for tool in &policy.allowed_tools {
        let required = required_caps_of(tool)
            .ok_or_else(|| RolePolicyCompileError::UnknownToolName { name: tool.clone() })?;
        names.insert(ToolName::new(tool));
        caps = caps | required;
    }
    if !policy.capabilities.is_empty() {
        let mut declared = ToolCapabilities::empty();
        for cap in &policy.capabilities {
            let parsed = ToolCapability::parse(cap)
                .ok_or_else(|| RolePolicyCompileError::UnknownCapability { name: cap.clone() })?;
            declared = declared | ToolCapabilities::single(parsed);
        }
        caps = caps & declared;
    }
    Ok(ToolProfile::baseline_with_names(caps, names))
}
```

依赖前置：若 `ToolCapability` 无 `parse(&str) -> Option<Self>`，在 `published_language.rs` 补（工具名/capability 字符串表驱动，含单测）；若 `ToolCapabilities` 无 `empty()`，补 `empty()` 与 `BitOr`/`BitAnd`（位集运算已有则复用）。

- [ ] **Step 4: 运行测试通过**：`cargo test -p aemeath-tools role_policy`
- [ ] **Step 5: Commit**：`git commit -m "feat(tools): RolePolicyConfig 编译为携带名单的 ToolProfile"`

---

### Task 4: 注册池统一 + sub-agent-restricted 显式名单（等价迁移）

**Files:**
- Modify: `agent/features/tools/src/adapters/registry.rs`（`builtin!` 调用区 85-265、`profile_for` 34-52、characterization 测试 386-425）
- Modify: `agent/features/tools/src/adapters/composition.rs`（`wire_builtin_catalog_execution` 74-102）

- [ ] **Step 1: 先固化等价基线测试**（改 `sub_agent_scope_characterization_is_exact`）

改造前先把现状 sub 工具集写成常量断言（该测试已存在，确认其断言的名单内容并保留），新增：

```rust
#[test]
fn sub_agent_restricted_profile_carries_legacy_sub_toolset_explicitly() {
    // 注册池扩大为全量后，等价迁移由 profile 显式名单保证
    let registry = Arc::new(ToolRegistry::new());
    // ...（复用 assembled_scope 测试装配）...
    let scope = assembled_scope(BuiltinRegistryScope::SubAgent);
    // 注册池 = 全量工具（sub 布尔退役）
    let main_scope = assembled_scope(BuiltinRegistryScope::Main);
    assert_eq!(scope.len(), main_scope.len(), "sub registration pool must equal main pool");
    // restricted profile 名单 = 原 sub 静态名单
    let legacy_sub_names: BTreeSet<&str> = [
        "Read", "Grep", "Glob", "Write", "Edit", "Bash", "WebFetch", "WebSearch",
        "Skill", "Memory", "Brief", "ToolSearch", "TaskGet", "TaskListGet", "TaskLists",
        "ListMcpResources", "ReadMcpResource",
    ].into_iter().collect();
    // 以改造前 sub_agent_scope_characterization_is_exact 断言的实际名单为准——
    // 执行此 task 时先读该测试断言，把常量替换为真实集合，NEVER 凭记忆写。
    let profile = profile_for(BuiltinRegistryScope::SubAgent, &ToolProfile::baseline(ToolCapabilities::all()));
    let names: BTreeSet<String> = profile.allowed_tool_names().unwrap().iter().map(|n| n.to_string()).collect();
    for name in legacy_sub_names { assert!(names.contains(name), "{name} missing from restricted profile"); }
    for absent in ["Agent", "AskUserQuestion", "TaskCreate", "TaskUpdate", "EnterWorktree", "ExitWorktree", "EnterPlanMode", "ExitPlanMode"] {
        assert!(!names.contains(absent), "{absent} must stay out of sub profile");
    }
}
```

- [ ] **Step 2: 运行确认失败**（profile 尚无名单）

- [ ] **Step 3: 实现**

1. `registry.rs`：所有 `builtin!(name, caps, true, false)`（sub=false 的 12 处：Agent/Bash/ExitWorktree/TaskCreate/TaskUpdate/TaskBlockBy/AskUserQuestion/EnterPlanMode/ExitPlanMode/EnterWorktree/TaskListComplete/TaskStop…以文件实际为准）改为 `true, true`；`belongs_to` 函数删除布尔分发，两个 scope 注册同一全量名单。`profile_for(SubAgent, main_parent)` 改为：

```rust
BuiltinRegistryScope::SubAgent => ToolProfile::baseline_with_names(
    main_parent.allowed_capabilities(),
    sub_agent_legacy_toolset(),
),
```

`sub_agent_legacy_toolset()` 返回改造前 sub=true 的工具名 `BTreeSet`（从 `builtin!` 原始布尔抄录，与 Step 1 常量同源——常量只定义一次，测试引用它，DRY）。

2. `composition.rs` `wire_builtin_catalog_execution` 不变（profile_for 内部已带名单）。

3. 更新 `full_scope_characterization_is_exact` / `sub_agent_scope_characterization_is_exact`：后者语义变为"注册池全量 + profile 名单等价"（引用同一常量断言）。

- [ ] **Step 4: 运行测试通过**：`cargo test -p aemeath-tools`
- [ ] **Step 5: Commit**：`git commit -m "refactor(tools): sub 注册池统一为全量，等价迁移由 restricted profile 显式名单承载"`

---

### Task 5: composition — per-role profiles 装配

**Files:**
- Modify: `agent/features/tools/src/adapters/composition.rs`（`wire_builtin_catalog_execution`）
- Modify: `agent/composition/src/runtime.rs`（305 行区调用点，含 `wire_builtin_catalog_execution` 的实际调用处）
- Test: `agent/features/tools/src/adapters/composition.rs` 内联 tests 或 `agent/composition/src/runtime_tests.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn wire_builtin_assembles_role_profiles_from_merged_roles() {
    // wiring 构建：内置+config 合并 roles → role:<name> profile 可查询
    // 用 TestCatalogExecutionFactory 装配后 snapshot("sub-agent", "role:searcher")
    // 不含 Write/Agent；snapshot("sub-agent", "sub-agent-restricted") 等价现状。
    // 具体装配调用以 wire_builtin_catalog_execution 新签名为准（见 Step 3）。
}
```

（测试体在实现签名时补全：断言 `role:searcher` snapshot 工具集 = searcher 名单 ∩ 注册池，且 `role:coder` 不含 `Agent`。）

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

`wire_builtin_catalog_execution` 增参：

```rust
pub fn wire_builtin_catalog_execution(
    task_access: Arc<dyn task::TaskAccess>,
    memory_source: Arc<dyn crate::domain::MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
    skill_loader: Arc<dyn crate::domain::SkillLoadPort>,
    role_policies: Vec<(String, share::config::RolePolicyConfig)>,
) -> Result<CatalogExecutionWiring, ToolBackingError> {
    // ...现有 scopes 装配循环...
    for (role_name, policy) in &role_policies {
        let profile = compile_role_profile(policy, &|tool| {
            registry.get(tool).map(|t| /* required caps 由 scope spec 提供而非 tool 实例 */)
        })?;
        // 注：required capabilities 查询走 main scope 的 ToolRegistrationSpec
        // （scope.get(&ToolName::new(tool))），闭包捕获已装配 main scope。
        profiles.insert(role_profile_name(role_name), profile);
    }
    wire_catalog_execution(registry, scopes, profiles)
}
```

role profile 与 main-full 的防提权：`compile_role_profile` 产出的 caps 来自注册池 required caps ∩ 声明，天然 ⊆ main-full（all），名单 ⊆ 注册池即 ⊆ main-full；无需再 derive（注册池=main-full 全量，编译函数已保证只收缩）。

`agent/composition/src/runtime.rs` 调用点：从 `ConfigSnapshot.agents()` 取 `merged_roles()`，过滤出带 policy 的条目传参。**合并逻辑单一事实源在 config**（Task 1 `merged_roles`），composition 只消费。

- [ ] **Step 4: 运行测试通过**：`cargo test -p aemeath-tools -p aemeath-composition`
- [ ] **Step 5: Commit**：`git commit -m "feat(composition): 按 merged roles 装配 role:<name> 工具 profile"`

---

### Task 6: runtime — resolve_derived_role 扩展 + select_tool_catalog 按 role 选 profile

**Files:**
- Modify: `agent/features/runtime/src/application/run/context_factory.rs`（`resolve_derived_role` 627-650、`select_tool_catalog` 483-502、provider 选择 389-397）
- Test: `agent/features/runtime/src/application/run/context_factory_tests.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn select_tool_catalog_uses_role_profile_when_policy_present() {
    // searcher role 带 policy → snapshot 请求 profile 为 role:searcher
    // 伪 catalog 记录收到的 (scope, profile) 参数并断言
}

#[test]
fn select_tool_catalog_falls_back_to_restricted_without_policy() {
    // 自定义 role 无 policy → sub-agent/sub-agent-restricted（现状）
}

#[test]
fn builtin_role_with_empty_model_falls_back_to_default_model() {
    // role.model 为空（内置 role 未被 config 覆盖 model）→ 使用 agents.default_model
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

1. `resolve_derived_role`：查找来源从 `config.agents().roles` 改为 `config.agents().merged_roles()`（内置 fallback + 覆盖）；`role.model` 为空时用 `config.agents().default_model`（仍空则维持现有 `SubUnknownModel` 错误，消息补充说明）。
2. `select_tool_catalog`：

```rust
let profile_name = match role.policy.as_ref() {
    Some(_) => tools::role_profile_name(&request.spec().name),
    None => ToolProfileName::new("sub-agent-restricted"),
};
let snapshot = parent.context().tool_catalog()
    .snapshot(&RegistryScopeName::new("sub-agent"), &profile_name)
    .map_err(...)?;
```

（`role` 经由 `RunCreationBindings`/`request` 传入 `select_tool_catalog`——把 `resolve_derived_role` 的结果缓存进创建流程（该函数已被 provider 选择调用，提取到 `RunCreationRequest` 解析阶段复用，避免双解析；以现有 creation 流程结构为准，NEVER 重复解析两次。）

3. `RestrictedToolCatalog`（40-52 行）的 guard（只服务 "sub-agent"/"sub-agent-restricted"）放宽为前缀匹配 `sub-agent` scope + `role:` 或 `sub-agent-restricted` profile。

- [ ] **Step 4: 运行测试通过**：`cargo test -p aemeath-runtime`
- [ ] **Step 5: Commit**：`git commit -m "feat(runtime): sub run 按 role 解析工具 profile，内置 role 模型回退 default_model"`

---

### Task 7: Agent 工具 role 枚举与校验对齐 merged roles

**Files:**
- Modify: `agent/features/tools/src/adapters/agent_tool.rs`（role 参数校验 62-63、80 注释）
- Modify: runtime 侧 unknown-role 校验点（`agent/features/tools/src/domain/agent_port.rs` 消费链；grep `roles` 定位实际校验函数）
- Test: `agent/features/tools/src/adapters/agent_tool_tests.rs`

- [ ] **Step 1: 写失败测试**：Agent 工具以内置 role 名（如 `searcher`）调用且 config 未定义该 role → 不报 unknown-role，正常派发（经 merged roles 解析）。
- [ ] **Step 2: 确认失败**
- [ ] **Step 3: 实现**：role 合法性校验来源改为 merged roles（config 层 API）；Agent 工具描述文本提及内置 role fallback。系统提示中的可用 role 列表（`prompt_build_ext.rs` 消费 `roles` 处）同步改用 `merged_roles()`，让 main LLM 看到内置 role。
- [ ] **Step 4: `cargo test -p aemeath-tools -p aemeath-runtime`**
- [ ] **Step 5: Commit**：`git commit -m "feat(tools): Agent 工具 role 校验与提示枚举消费 merged roles"`

---

### Task 8: 全链路验证 + 门禁闭合 + PR

- [ ] **Step 1: 全量验证**

```bash
cargo fmt --check && cargo test --workspace && cargo clippy --workspace -- -D warnings
```

- [ ] **Step 2: 场景冒烟（TUI 手测）**：`AEMEATH_LOG_LEVEL=debug cargo run`，派发 searcher sub-agent，确认其工具列表无 Write/Agent；派发无 policy 自定义 role，确认行为同现状。
- [ ] **Step 3: 门禁核对**：逐项核对 issue #1658 Scope/验收 checklist，全部闭合。
- [ ] **Step 4: 同步文档**：`specs/3.5-tools.md`、`specs/3.9-config-compat.md` 若涉及 role/工具装配描述则补一行引用 `03-role-policy.md`（改动最小化，仅引用不复制）。
- [ ] **Step 5: PR**：`git pull origin main` 后 push 分支，PR 引用 `Closes #1658`，按 `.github/pull_request_template.md` 填 Summary/Refs/Breaking change/Test plan。

---

## Self-Review 结论

- **Spec 覆盖**：issue #1658 六项 Scope → Task 1（config+内置）、Task 2/3（ToolProfile/编译）、Task 4（静态布尔退役+等价迁移）、Task 5/6（装配+runtime 选择）、Task 7（Agent 工具对齐）；验收四项 → Task 1/2/4 测试、Task 8。无缺口。
- **占位符**：Task 4 Step 1 的 legacy 名单常量明确标注"以改造前测试断言为准，NEVER 凭记忆写"；Task 5/6 测试骨架标注"以实际签名为准补全"——执行者必须先读对应文件再落笔，属于防走样约束而非偷懒占位。
- **类型一致性**：`compile_role_profile(policy, &dyn Fn)`、`role_profile_name(&str) -> ToolProfileName`、`derive_restricted(parent, caps, Option<BTreeSet<ToolName>>)` 三处签名在 Task 2/3/5/6 间一致；`merged_roles()` 单一事实源在 config，composition/runtime/tools 三处消费。
