# Skill Session Revision Dedup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 Main Session / Sub-agent instance 作用域持久化 Skill 内容 revision，未更新时返回已加载提示，更新后返回完整新正文。

**Architecture:** Tools BC 发布 `SkillLoadScope`、compare-and-record Published Language 与窄状态端口；Context Management 以 `CanonicalSession` 和统一 mutation gate 实现唯一 durable backing；Runtime 只装配稳定主体 scope，并把 Main 的 Context 状态端口传给 Main 与所有派生 Sub-agent。Skill Tool 仍先按调用时环境读取当前正文，再依据原子判定选择完整正文或 already-loaded 提示。

**Tech Stack:** Rust 2021、async-trait、Serde/JSON、Tokio、AtomicBlob Session、SHA-256、Cargo workspace。

---

## 文件结构

- `agent/features/tools/src/domain/skill_state.rs`：Skill 加载作用域、mutation、判定、错误与状态端口。
- `agent/features/tools/src/domain/skill_state_tests.rs`：Tools-owned Published Language 等价类测试。
- `agent/features/tools/src/domain/{rs,context.rs}`、`agent/features/tools/src/lib.rs`：公开窄类型并把状态 binding 注入 Tool execution context。
- `agent/features/tools/src/adapters/skill_tool.rs`、`skill_tool_contract_tests.rs`：调用 loader 后执行 compare-and-record，并选择正文或提示。
- `agent/features/context/src/domain/session/envelope.rs`、`envelope_tests.rs`：CanonicalSession 加载记录 aggregate 规则。
- `agent/features/context/src/ports/{context_port.rs,rs}`：ContextPort / SessionRepository 原子 mutation OHS。
- `agent/features/context/src/application/service.rs`：ContextApplicationService 委托 mutation。
- `agent/features/context/src/adapters/{canonical_session.rs,in_memory_session.rs}`：durable 与测试/isolated backing 实现。
- `agent/features/context/tests/{session_envelope_codec.rs,main_session_wiring.rs}`：schema v6、旧版兼容、写失败、reopen/Resume。
- `agent/features/runtime/src/application/skill_load_state.rs`：把 ContextPort 适配为 Tools-owned `SkillLoadStatePort`。
- `agent/features/runtime/src/application/runtime_context.rs` 及 factory/bindings：保存并继承唯一 Skill state port。
- `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`：Main scope 装配。
- `agent/features/runtime/src/application/subagent/runner/{setup.rs,tests/runtime_context_wiring.rs}`：生成稳定 Sub-agent instance scope并向嵌套派生链继承同一 state port。
- `docs/design/**`、`specs/**`：同步当前实现与测试矩阵。

### Task 1: 发布 Tools-owned Skill 状态语言与执行 binding

**Files:**
- Create: `agent/features/tools/src/domain/skill_state.rs`
- Create: `agent/features/tools/src/domain/skill_state_tests.rs`
- Modify: `agent/features/tools/src/domain.rs`
- Modify: `agent/features/tools/src/domain/context.rs`
- Modify: `agent/features/tools/src/lib.rs`

- [ ] **Step 1: 写 Published Language 失败测试**

在 `skill_state_tests.rs` 覆盖：`Main` 不含 Run identity；相同/不同 Sub-agent instance 的相等性；空 canonical name/revision/instance id 被构造器拒绝；mutation 完整保留 session、scope、name、revision。

```rust
#[test]
fn main_scope_is_stable_and_subagent_scope_uses_instance_identity() {
    assert_eq!(SkillLoadScope::main(), SkillLoadScope::main());
    assert_eq!(
        SkillLoadScope::subagent("agent-1").unwrap(),
        SkillLoadScope::subagent("agent-1").unwrap()
    );
    assert_ne!(
        SkillLoadScope::subagent("agent-1").unwrap(),
        SkillLoadScope::subagent("agent-2").unwrap()
    );
}

#[test]
fn mutation_rejects_blank_identity_fields() {
    assert!(SkillLoadMutation::new("session", SkillLoadScope::main(), "", "r1").is_err());
    assert!(SkillLoadMutation::new("session", SkillLoadScope::main(), "review", "").is_err());
    assert!(SkillLoadScope::subagent(" ").is_err());
}
```

- [ ] **Step 2: 运行测试确认因模块/类型不存在而失败**

Run: `cargo test -p tools skill_state -- --nocapture`
Expected: FAIL，错误包含 unresolved module/type `skill_state` / `SkillLoadScope`。

- [ ] **Step 3: 实现最小 Published Language 与端口**

`skill_state.rs` 定义：

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "instance_id", rename_all = "snake_case")]
pub enum SkillLoadScope {
    Main,
    Subagent(String),
}

impl SkillLoadScope {
    pub const fn main() -> Self { Self::Main }
    pub fn subagent(value: impl Into<String>) -> Result<Self, SkillLoadStateError> {
        let value = value.into();
        if value.trim().is_empty() { return Err(SkillLoadStateError::InvalidInstanceId); }
        Ok(Self::Subagent(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoadMutation {
    pub session_id: String,
    pub scope: SkillLoadScope,
    pub skill_name: String,
    pub revision: String,
}

impl SkillLoadMutation {
    pub fn new(
        session_id: impl Into<String>, scope: SkillLoadScope,
        skill_name: impl Into<String>, revision: impl Into<String>,
    ) -> Result<Self, SkillLoadStateError> {
        let value = Self { session_id: session_id.into(), scope, skill_name: skill_name.into(), revision: revision.into() };
        if value.session_id.trim().is_empty() { return Err(SkillLoadStateError::InvalidSessionId); }
        if value.skill_name.trim().is_empty() { return Err(SkillLoadStateError::InvalidSkillName); }
        if value.revision.trim().is_empty() { return Err(SkillLoadStateError::InvalidRevision); }
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillLoadDecision { Fresh, Updated, AlreadyLoaded }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillLoadStateError {
    #[error("Session identity 无效")] InvalidSessionId,
    #[error("Sub-agent instance identity 无效")] InvalidInstanceId,
    #[error("Skill canonical identity 无效")] InvalidSkillName,
    #[error("Skill revision 无效")] InvalidRevision,
    #[error("Session 不存在: {0}")] SessionNotFound(String),
    #[error("Skill 加载状态持久化失败: {0}")] Storage(String),
}

#[async_trait]
pub trait SkillLoadStatePort: Send + Sync {
    async fn compare_and_record(
        &self, mutation: SkillLoadMutation,
    ) -> Result<SkillLoadDecision, SkillLoadStateError>;
}
```

在 `ToolExecutionPorts` 增加可选 `skill_load_state` 和 `with_skill_load_state`；在 `ToolExecutionContext` 暴露 clone accessor。公开这些类型，保持 Tools 不依赖 Context。

- [ ] **Step 4: 运行 Tools domain 测试**

Run: `cargo test -p tools skill_state -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add agent/features/tools/src/domain/skill_state.rs agent/features/tools/src/domain/skill_state_tests.rs agent/features/tools/src/domain.rs agent/features/tools/src/domain/context.rs agent/features/tools/src/lib.rs
git commit -m "feat(tools): #1460 publish skill load state contract"
```

### Task 2: 建立 Context-owned Canonical Session 原子状态

**Files:**
- Modify: `agent/features/context/src/domain/session/envelope.rs`
- Modify: `agent/features/context/src/domain/session/envelope_tests.rs`
- Modify: `agent/features/context/src/domain/session.rs`
- Modify: `agent/features/context/src/ports/context_port.rs`
- Modify: `agent/features/context/src/ports.rs`
- Modify: `agent/features/context/src/application/service.rs`
- Modify: `agent/features/context/src/adapters/canonical_session.rs`
- Modify: `agent/features/context/src/adapters/in_memory_session.rs`
- Test: `agent/features/context/tests/session_envelope_codec.rs`
- Test: `agent/features/context/tests/main_session_wiring.rs`

- [ ] **Step 1: 写 aggregate、codec 与原子写失败测试**

覆盖：

```rust
#[test]
fn canonical_session_compares_and_records_scope_revision() {
    let mut session = CanonicalSession::fixture("session");
    let scope = SkillLoadScope::main();
    assert_eq!(session.compare_and_record_skill(&scope, "brainstorming", "r1"), SkillLoadDecision::Fresh);
    assert_eq!(session.compare_and_record_skill(&scope, "brainstorming", "r1"), SkillLoadDecision::AlreadyLoaded);
    assert_eq!(session.compare_and_record_skill(&scope, "brainstorming", "r2"), SkillLoadDecision::Updated);
    assert_eq!(session.loaded_skill_revision(&scope, "brainstorming"), Some("r2"));
}

#[test]
fn v5_session_upgrades_with_empty_skill_load_records() {
    let mut value: serde_json::Value = serde_json::from_slice(
        &SessionCodec::encode(&CanonicalSession::fixture("v5")).unwrap()
    ).unwrap();
    value["schema_version"] = serde_json::json!(5);
    value.as_object_mut().unwrap().remove("skill_load_records");
    let decoded = decode_session(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(decoded.upgraded_from_legacy);
    assert!(decoded.session.skill_load_records.is_empty());
}
```

在 repository 测试中用 failing writer 断言 compare-and-record 返回 Storage，committed session revision 和 records 不变；相同 revision 断言 writer 未被调用、Session revision 不增加；两个并发请求最多一个 `Fresh`。

- [ ] **Step 2: 运行定向测试确认失败**

Run: `cargo test -p context skill_load -- --nocapture`
Expected: FAIL，缺少 `skill_load_records` / compare-and-record port。

- [ ] **Step 3: 实现 Session 记录与 schema v6**

在 envelope 增加：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLoadRecord {
    pub scope: tools::SkillLoadScope,
    pub skill_name: String,
    pub revision: String,
}
```

`CanonicalSession` 增加 `#[serde(default)] pub skill_load_records: Vec<SkillLoadRecord>`。按 `(scope, skill_name)` 查找；Fresh/Updated 改 candidate，AlreadyLoaded 不改。将 `CURRENT_SESSION_SCHEMA_VERSION` 提升至 6；v2-v5 reader 全部升级空 records，current writer 输出 v6，future version规则不变。

- [ ] **Step 4: 实现 ContextPort / SessionRepository mutation**

两个 trait 增加 `compare_and_record_skill_load`。`ContextApplicationService` 只委托 repository。`CanonicalSessionRepository` 在 mutation gate 内：校验 session、clone current、执行 aggregate compare；AlreadyLoaded 直接返回；Fresh/Updated 时 revision +1、更新时间、收集 Task/Workspace snapshot、save candidate、publish generation。`InMemorySessionRepository` 用同一键和 mutex 保证测试/isolated 路径语义一致。

- [ ] **Step 5: 运行 Context 测试**

Run: `cargo test -p context skill_load -- --nocapture && cargo test -p context session_envelope_codec -- --nocapture`
Expected: PASS；future schema 与 v1-v5 兼容测试均通过。

- [ ] **Step 6: 提交**

```bash
git add agent/features/context
git commit -m "feat(context): #1460 persist skill revisions atomically"
```

### Task 3: 让 Skill Tool 消费原子判定

**Files:**
- Modify: `agent/features/tools/src/adapters/skill_tool.rs`
- Modify: `agent/features/tools/src/adapters/skill_tool_contract_tests.rs`

- [ ] **Step 1: 写 Tool 行为失败测试**

使用 scripted `SkillLoadStatePort` 覆盖 `Fresh` 返回正文、`Updated` 返回正文、`AlreadyLoaded` 返回固定提示且 structured metadata 仍含 name/revision、state error 不泄漏正文并返回 Failure、缺少 binding fail-closed。

```rust
assert_eq!(already_loaded_success.content[0].text,
    "Skill superpowers:brainstorming 已加载，内容未更新（revision: r1）。请继续使用已有指令。");
assert!(!already_loaded_success.content[0].text.contains("BODY_SENTINEL"));
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p tools skill_tool -- --nocapture`
Expected: FAIL，当前 Tool 总是返回 `loaded.content()`。

- [ ] **Step 3: 实现 Skill Tool 判定**

loader 成功后，从 execution context 取得 `SkillLoadStatePort`；用 `ctx.scope` 外显提供的 session id 与显式 `SkillLoadScope` 生成 mutation。先 await compare-and-record：Fresh/Updated 返回正文；AlreadyLoaded 返回提示；任何 state error 走安全中文 Failure。结构化结果新增 typed decision 字段但不包含正文。

- [ ] **Step 4: 运行 Tools 全部测试**

Run: `cargo test -p tools --lib`
Expected: PASS，既有 schema 仍仅包含 `skill`。

- [ ] **Step 5: 提交**

```bash
git add agent/features/tools/src/adapters/skill_tool.rs agent/features/tools/src/adapters/skill_tool_contract_tests.rs
git commit -m "feat(tools): #1460 suppress unchanged skill bodies"
```

### Task 4: Runtime 装配 Main 与稳定 Sub-agent instance scope

**Files:**
- Create: `agent/features/runtime/src/application/skill_load_state.rs`
- Create: `agent/features/runtime/src/application/skill_load_state_tests.rs`
- Modify: `agent/features/runtime/src/application.rs`
- Modify: `agent/features/runtime/src/application/runtime_context.rs`
- Modify: `agent/features/runtime/src/application/runtime_context_factory.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/setup.rs`
- Test: `agent/features/runtime/src/application/subagent/runner/tests/runtime_context_wiring.rs`
- Test: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`

- [ ] **Step 1: 写 Runtime wiring 失败测试**

断言 Main 两个不同 run_id 的 ToolExecutionContext 都携带 `SkillLoadScope::Main`；同一 `DerivedSubRun` 的 scope instance id 稳定；两个独立 derive 得到不同 instance id；nested Sub 派生继承与 Main 相同的 state port backing，而不是 isolated Context backing；源码/结构断言禁止从 `run_id` 构造 scope。

- [ ] **Step 2: 运行定向测试确认失败**

Run: `cargo test -p runtime skill_load_state -- --nocapture`
Expected: FAIL，RuntimeContext 尚无该 capability。

- [ ] **Step 3: 实现 Context→Tools ACL adapter**

`skill_load_state.rs` 定义持有 `Arc<dyn ContextPort>` 的 adapter，实现 Tools port并原样转发 typed mutation；不读取 CanonicalSession，不缓存。

- [ ] **Step 4: 将 state port 纳入 RuntimeContext 能力**

`RuntimeContext` 增加私有 `Arc<dyn tools::SkillLoadStatePort>` 与 accessor；Main factory 从 Main `ContextPort` 构造 adapter；Sub factory显式继承 parent 的 state port。不要从 Sub 的 `isolated_context_with_skill` 创建新的 durable truth。

- [ ] **Step 5: 装配稳定 scope**

Main build agent 使用 `SkillLoadScope::Main`。`derive_sub_run` 在创建 Sub-agent instance 时生成一次 UUIDv7 identity，存入 `DerivedSubRun`；`run_agent` 将该 scope 注入该实例的 ToolExecutionPorts。后续同一 SubAgentRun 的多个 step/tool round 复用同一 ToolExecutionContext。新 derive 得到新 identity。

- [ ] **Step 6: 运行 Runtime wiring 和 crate 测试**

Run: `cargo test -p runtime skill_load_state -- --nocapture && cargo test -p runtime --lib`
Expected: PASS。

- [ ] **Step 7: 提交**

```bash
git add agent/features/runtime
git commit -m "feat(runtime): #1460 bind stable agent skill scopes"
```

### Task 5: 完成跨层 Resume、更新与并发场景

**Files:**
- Modify: `agent/features/context/tests/main_session_wiring.rs`
- Modify: `agent/features/tools/src/adapters/skill_tool_contract_tests.rs`
- Modify: `agent/composition/tests/main_session_wiring.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/tests/runtime_context_wiring.rs`

- [ ] **Step 1: 写跨层失败场景**

建立真实临时 Skill 文件与 AtomicBlob Session：Main 第一次执行返回正文；更换 run_id 再执行返回 already-loaded；reopen/resume 后仍 already-loaded；修改正文后返回新正文且 revision 更新；并发两个相同调用只有一个结果含正文。分别构造两个 Sub instance，断言各自首次有正文，同一实例后续无正文。

- [ ] **Step 2: 运行场景确认任一断点会失败**

Run: `cargo test -p composition --test main_session_wiring skill_load -- --nocapture`
Expected: FAIL，直到跨层 wiring 完整。

- [ ] **Step 3: 仅补齐相邻边界缺口**

修复测试揭示的 session id、scope、canonical name、revision 字段遗漏；不得新增 Runtime cache、历史 Tool Result 扫描或第二 writer。

- [ ] **Step 4: 运行跨层矩阵**

Run: `cargo test -p tools --lib && cargo test -p context --all-targets && cargo test -p runtime --lib && cargo test -p composition --tests`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add agent/features/tools agent/features/context agent/features/runtime agent/composition
git commit -m "test: #1460 cover skill dedup resume and isolation"
```

### Task 6: 同步设计文档、规范与验证门禁

**Files:**
- Modify: `docs/design/02-modules/tools/02-ports-and-lifecycle.md`
- Modify: `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
- Modify: `docs/design/02-modules/context-management/01-session.md`
- Modify: `docs/design/03-engineering/04-testing-and-coverage.md`
- Modify: `specs/3.4-runtime.md`
- Modify: `specs/3.5-tools.md`
- Modify: `specs/3.10-storage.md`

- [ ] **Step 1: 更新 Current/Target 文档**

写明：Tools 发布状态 PL 但不持久化；Runtime 使用 Main/Sub-agent instance scope 且禁止 run_id 推导；Context v6 保存唯一 revision records；Storage 仍只提供 AtomicBlob；测试矩阵加入跨 Run、Resume、更新、隔离、并发证据。

- [ ] **Step 2: 检查旧路径与旁路**

Run: `rg 'run_id.*SkillLoadScope|SkillLoadScope.*run_id|skill.*HashMap|skills_map' agent/features/{tools,runtime,context}`
Expected: 不存在以 run_id 去重、进程内生产缓存或历史结果反查；测试 fixture 命中需逐项解释。

- [ ] **Step 3: 运行格式与定向门禁**

Run: `cargo fmt --check`
Expected: PASS。

Run: `cargo clippy -p tools -p context -p runtime -p composition --all-targets --all-features -- -D warnings`
Expected: PASS。

Run: `.agents/hooks/check-architecture-guards.sh --full`
Expected: PASS。

- [ ] **Step 4: 运行 workspace 验证**

Run: `cargo test --workspace`
Expected: PASS；若时间预算阻断，必须报告实际停止位置，不能宣称完整通过。

- [ ] **Step 5: 更新 Issue checklist 与提交文档**

```bash
git add docs/design specs
git commit -m "docs(design): #1460 align skill dedup lifecycle"
```

- [ ] **Step 6: 最终审计**

Run: `git diff origin/main...HEAD --check && git status --short && git log --oneline origin/main..HEAD`
Expected: 无 whitespace error、工作树干净、提交只属于 #1460。
