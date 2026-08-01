# RuntimeContext 单一装配链与分层守卫实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. 本计划不得调用 Agent；由当前会话在既有隔离 worktree 内逐项执行并保留首次验证结果。

**Goal:** 删除 RuntimeContext 的测试并行装配算法，使 Session Run、Derived Run 与相关测试都经 `RunCreationRequest → RunFactory::create → RunInstance → RunLauncher::launch → 单一 Loop Engine`，并建立 L0–L4 无旁路证据。

**Architecture:** `RuntimeContextFactory::prepare` 保持唯一 crate-private Context 装配算法且只由 `RunFactory::create` 调用；测试 fixture 只组合 typed `SessionRunBindings` / `ParentRunBindings`，最终返回完整 `RunInstance`。Guard 以允许调用点和结构不变量为核心，L1 验证纯值/ceiling，L2 验证 Runtime 装配，L3 验证 Composition 注入，L4 验证 Main/Derived 最终旅程。

**Tech Stack:** Rust、Tokio、Runtime application/domain、Composition Root、Bash/Python architecture guards、Cargo L0–L4 验证。

**Design:** `docs/superpowers/specs/2026-08-01-runtime-context-single-assembly-design.md`

---

## 0. 文件职责地图

### 新增

- `agent/features/runtime/src/application/run/tests/run_factory_support.rs`
  - `cfg(test)` owning-layer fixture 入口。
  - 显式位于 `tests/` 物理边界，避免生产 Guard 将 fixture 当作生产 Session/Task/Config 装配。
  - 只负责组合测试服务、Session/Parent typed bindings 与 production `RunFactory`。
  - 不定义第二套 capability selector，不调用 `RuntimeContextFactory::prepare` 或 `RuntimeContext::new`。
- `agent/features/runtime/src/application/run/tests/run_factory_support/fakes.rs`
  - Runtime Run 装配测试所需的职责型 Fake/Spy Port。
- `agent/features/runtime/src/application/run/tests/run_factory_support/session_run.rs`
  - 构造 `SessionState`、`SessionRunBindings` 与 Session `RunInstance`。
- `agent/features/runtime/src/application/run/tests/run_factory_support/derived_run.rs`
  - 从 production parent context/workspace capability 构造 `ParentRunFacts`、`ParentRunBindings` 与 Derived `RunInstance`。
- `agent/features/runtime/src/application/run/scenario_tests.rs`
  - L4 Main/Derived 单一创建链场景入口。
- `agent/features/runtime/src/application/run/scenario_tests/main_run.rs`
  - Main `RunInstance → RunLauncher → Loop` 场景。
- `agent/features/runtime/src/application/run/scenario_tests/derived_run.rs`
  - Derived `parent → restricted RunInstance → same launcher/Loop` 场景。

### 修改

- `agent/features/runtime/src/application/run.rs`
  - 接入 `cfg(test)` fixture 与 L4 场景模块。
- `agent/features/runtime/src/application/run/context.rs`
  - 删除 `RuntimeContextAssemblyToken::new_for_test`；修正唯一 factory 文档。
- `agent/features/runtime/src/application/run/context_factory.rs`
  - 删除 test-only `create`、测试 selector；只保留生产 `prepare` 与可复用的 factory service 替换能力。
- `agent/features/runtime/src/application/run/factory.rs`
  - 保持唯一完整 Run 创建入口；必要时增加仅限 owning layer 的测试可观测契约，不扩大生产 façade。
- `agent/features/runtime/src/application/run/creation.rs`
  - 保持 pure-value request/facts 与 live bindings 分离；测试只使用现有窄 accessor。
- `agent/features/runtime/src/application/run/context_factory_tests.rs`
  - 改为 L2 production-chain 装配测试，删除 selector/direct-create 测试。
- `agent/features/runtime/src/application/run/context_tests.rs`
  - 通过 fixture 返回的 `RunInstance::context()` 测试 Context 行为。
- `agent/features/runtime/src/application/run/derived/tests/runtime_context_derivation.rs`
  - 删除手填 `RunCapabilityBindings` 的 parent context helper，统一从 parent `RunInstance` 派生。
- `agent/features/runtime/src/application/run/derived/tests/runtime_context_wiring.rs`
  - 复用同一 fixture，保留 Derived tool/policy/cancel 相邻链路断言。
- `agent/features/runtime/src/application/client/from_args.rs`
  - 测试 helper 改走 production RunFactory；不改 bootstrap 业务行为。
- `agent/features/runtime/src/application/loop_engine/chat/pre_compact_trigger_tests.rs`
  - Context fixture 改走 production RunFactory。
- `agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs`
  - 若 `with_hooks` 被删除，则改为在测试 factory 创建前注入 Hook service；不改 Loop 语义。
- `agent/composition/src/runtime.rs`
  - 删除外部追踪编号注释；保持同一 factory Arc 注入 bootstrap 与 agent runner。
- `agent/composition/src/runtime_tests.rs`
  - 增加 Composition 内 factory 单实例/字段无覆写契约。
- `agent/composition/tests/main_session_wiring.rs`
  - 增加 crate 公共边界可证明的 bootstrap wiring 契约。
- `.agents/hooks/check-runtime-capability-assembly.sh`
  - 增加唯一构造点、唯一 prepare 调用者、禁止 test-only Context creator、pure-value 与 façade 规则。
- `.agents/hooks/check-runtime-capability-assembly-tests.sh`
  - 增加每条新 Guard 的故意违规及总编排 `exit 2` 证明。
- `docs/design/03-engineering/01-architecture-guards.md`
  - 更新 Guard 注册说明与探针矩阵。
- `docs/design/03-engineering/04-testing-and-coverage.md`
  - 登记本能力 L0–L4 证据矩阵，并移除本次触及段落中的外部追踪编号引用。
- `docs/design/02-modules/runtime/07-runtime-ownership-and-assembly.md`
  - 将过期 `RuntimeContextFactory::create` 伪代码改成 `RunFactory::create → RuntimeContextFactory::prepare` 唯一链。

### 不修改

- 不改变 Run/Loop/Tool/Hook/Interaction 的业务语义。
- 不新增公开测试 Harness。
- 不导出 `RunFactory`、`RunInstance`、`SessionRunBindings` 或 `ParentRunBindings` 到 Runtime crate root。
- 不新增 `*Parts`、动态 locator 或新的 fat capability bundle。

---

## Task 1：记录开发环境与现有旁路基线

**Files:**
- Read: `docs/superpowers/specs/2026-08-01-runtime-context-single-assembly-design.md`
- Read: `agent/features/runtime/src/application/run/context_factory.rs`
- Read: `.agents/hooks/check-runtime-capability-assembly.sh`
- Read: `.agents/hooks/check-runtime-capability-assembly-tests.sh`

- [ ] **Step 1: 检查开发环境**

Run:

```bash
scripts/setup-dev-env.sh --check
```

Expected: `PASS`；若仅缺本机可选覆盖率组件，原样记录为非本阶段 blocker，不修改全局环境。

- [ ] **Step 2: 记录 Git 与 worktree 基线**

Run:

```bash
pwd
git status --short --branch
git rev-parse HEAD
git rev-parse origin/main
```

Expected: 位于 `refactor-1398-runtime-context-guard` worktree；只存在已批准设计与本计划的未提交文档改动。

- [ ] **Step 3: 记录测试旁路调用点**

Run:

```bash
rg -n 'RuntimeContextFactory::create|\.create\(&.*(?:bindings|make_bindings)|RuntimeContext::new\(|new_for_test\(' \
  agent/features/runtime/src/application
```

Expected: 输出当前 `context_factory_tests.rs`、`context_tests.rs`、Derived tests、`from_args.rs` 测试、pre-compact 测试中的 direct-create，以及 `context_factory.rs` test-only 构造。

- [ ] **Step 4: 记录现有 Guard 基线**

Run:

```bash
bash .agents/hooks/check-runtime-capability-assembly-tests.sh
bash .agents/hooks/check-runtime-capability-assembly.sh
```

Expected: 当前 baseline 通过；这证明旧 Guard 尚未把 test-only `create` 识别为违规，而不是证明结构已满足目标。

- [ ] **Step 5: 保存首次结果到当前 Task 记录**

Expected: 记录命令、退出码、旁路文件清单；失败不得用重跑成功覆盖。

---

## Task 2：先建立 L0 失败探针

**Files:**
- Modify: `.agents/hooks/check-runtime-capability-assembly-tests.sh`
- Test: `.agents/hooks/check-runtime-capability-assembly-tests.sh`

- [ ] **Step 1: 为 test-only Context creator 添加失败探针**

在临时 repo 的 `context_factory.rs` 追加等价违规：

```rust
#[cfg(test)]
impl RuntimeContextFactory {
    pub(crate) fn alternate_context_creator(&self) -> RuntimeContext {
        RuntimeContext::new(
            unreachable!(),
            unreachable!(),
            unreachable!(),
            RuntimeContextAssemblyToken::new(),
        )
    }
}
```

探针必须期望稳定诊断：

```text
RuntimeContext creation must not have a test-only alternate entry
```

- [ ] **Step 2: 为非 factory 的 `prepare` 调用添加失败探针**

在临时 `application/run/launcher.rs` 追加只用于文本 Guard 的违规调用，期望：

```text
RuntimeContextFactory::prepare has an unapproved caller
```

- [ ] **Step 3: 为非 factory 的 `RunInstance::new` 调用添加失败探针**

在临时 `application/run/launcher.rs` 追加违规调用，期望：

```text
RunInstance::new has an unapproved caller
```

- [ ] **Step 4: 为 Main/Derived 绕过添加失败探针**

分别临时删除：

```rust
run_factory.create(request)
run::launcher::launch(
```

以及：

```rust
run_factory.create(creation_request)
run::launcher::launch(instance
```

Expected diagnostics:

```text
Main Run must use RunFactory::create and RunLauncher::launch
Derived Run must use RunFactory::create
Derived Run must pass RunInstance to RunLauncher::launch
```

- [ ] **Step 5: 为 pure-value 与 façade 规则保留/补齐探针**

验证已有 `SessionSnapshot` live Port、crate-root 导出 `RunInstance`、fat supertrait/alias/test double 探针；新增 `RunCreationRequest` live Port 探针，期望：

```text
RunCreationRequest must contain pure values only
```

- [ ] **Step 6: 运行探针并确认先失败**

Run:

```bash
bash .agents/hooks/check-runtime-capability-assembly-tests.sh
```

Expected: FAIL，因为单 Guard 尚未实现新诊断；保留首次失败输出。

---

## Task 3：实现 L0 单 Guard 的结构白名单

**Files:**
- Modify: `.agents/hooks/check-runtime-capability-assembly.sh`
- Test: `.agents/hooks/check-runtime-capability-assembly-tests.sh`

- [ ] **Step 1: 让 Guard 同时扫描生产与测试 Context 构造**

实现允许位置集合：

```python
APPROVED_RUNTIME_CONTEXT_CONSTRUCTOR = FACTORY
APPROVED_PREPARE_CALLER = RUN_FACTORY
APPROVED_RUN_INSTANCE_CONSTRUCTOR = RUN_FACTORY
```

对 `RuntimeContext::new(` 不再跳过 test source；只有 `context_factory.rs` 生产 `bind_runtime_context` 中的调用允许。若 `context_factory.rs` 的 `cfg(test)` item 返回 `RuntimeContext` 或调用 `RuntimeContext::new`，报告：

```text
RuntimeContext creation must not have a test-only alternate entry
```

- [ ] **Step 2: 限定 token 构造点**

扫描所有 Runtime Rust source，`RuntimeContextAssemblyToken::new()` 只允许在 `context_factory.rs` 的生产区域出现；`new_for_test` 必须不存在。

Expected diagnostic:

```text
RuntimeContextAssemblyToken::new has an unapproved caller
```

- [ ] **Step 3: 限定 `prepare` 与 `RunInstance::new` 调用者**

扫描所有 Runtime Rust source：

```text
RuntimeContextFactory::prepare / .prepare
RunInstance::new
```

分别只允许 `application/run/factory.rs`。

- [ ] **Step 4: 保持结构规则不依赖退役名字**

保留 trait/supertrait/type alias/多 Port impl 的类别扫描；不要只新增 `RuntimeContextFactory::create` 名称黑名单。test-only alternate creator 通过“返回 `RuntimeContext` 或调用私有构造”的结构识别阻断。

- [ ] **Step 5: 运行单 Guard 探针**

Run:

```bash
bash .agents/hooks/check-runtime-capability-assembly-tests.sh
bash .agents/hooks/check-runtime-capability-assembly.sh
```

Expected: 两者 PASS；每个故意违规均返回 `exit 2` 且 baseline 恢复后通过。

- [ ] **Step 6: 运行总编排故意违规证明**

在 probe helper 中对至少一个新增违规同时执行：

```bash
AEMEATH_PROJECT_DIR="$TMP/repo" bash "$TMP/repo/.agents/hooks/check-architecture-guards.sh" --fast
```

Expected: `exit 2`，诊断包含同一稳定不变量；恢复 baseline 后 `--fast` PASS。

---

## Task 4：建立 production-chain 测试 fixture

**Files:**
- Create: `agent/features/runtime/src/application/run/tests/run_factory_support.rs`
- Create: `agent/features/runtime/src/application/run/tests/run_factory_support/fakes.rs`
- Create: `agent/features/runtime/src/application/run/tests/run_factory_support/session_run.rs`
- Create: `agent/features/runtime/src/application/run/tests/run_factory_support/derived_run.rs`
- Modify: `agent/features/runtime/src/application/run.rs`
- Test: `agent/features/runtime/src/application/run/context_factory_tests.rs`

- [ ] **Step 1: 先写 fixture 结构契约测试**

在 `context_factory_tests.rs` 增加：

```rust
#[test]
fn test_fixture_uses_the_production_run_factory_chain() {
    let fixture_source = include_str!("tests/run_factory_support.rs");
    let session_source = include_str!("tests/run_factory_support/session_run.rs");
    let derived_source = include_str!("tests/run_factory_support/derived_run.rs");

    for source in [fixture_source, session_source, derived_source] {
        assert!(!source.contains("RuntimeContext::new("));
        assert!(!source.contains(".prepare("));
        assert!(!source.contains("RunCapabilityBindings"));
    }
    assert!(session_source.contains("RunFactory::for_session"));
    assert!(session_source.contains(".create(request)"));
    assert!(derived_source.contains("RunFactory::for_parent"));
    assert!(derived_source.contains(".create(request)"));
}
```

- [ ] **Step 2: 运行结构测试确认失败**

Run:

```bash
cargo test -p runtime test_fixture_uses_the_production_run_factory_chain -- --exact
```

Expected: FAIL，因为 fixture 文件尚不存在或模块尚未接入。

- [ ] **Step 3: 新建职责型 Fake/Spy**

`testing/fakes.rs` 只定义职责明确的：

```rust
pub(crate) struct FakeContextPort;
pub(crate) struct FakeProviderPort;
pub(crate) struct FakeToolCatalog;
pub(crate) struct FakeToolExecution;
pub(crate) struct FakePolicyPort;
pub(crate) struct FakeReflectionHistory;
pub(crate) struct FakeHookPort;
pub(crate) struct RecordingEventSink;
```

每个类型只实现自身 Port；复用现有 `task::TaskStore`、`memory::NoOpMemory`、`InteractionBridge`，不复制万能 fake。

- [ ] **Step 4: 实现 Session fixture**

`tests/run_factory_support/session_run.rs` 提供：

```rust
pub(crate) struct SessionRunFixture {
    pub(crate) context_factory: Arc<RuntimeContextFactory>,
    pub(crate) session_state: SessionState,
    pub(crate) session_bindings: SessionRunBindings,
}

impl SessionRunFixture {
    pub(crate) fn create(&self, spec: RunSpec) -> Result<RunInstance, RunCreationError> {
        let request = RunCreationRequest::new(
            spec,
            self.session_state.snapshot_for_run(),
            None,
        )?;
        RunFactory::for_session(
            self.context_factory.clone(),
            self.session_bindings.clone(),
        )
        .create(request)
    }
}
```

fixture builder 必须构造真实 `MainSessionWiring` 或复用现有 Context test wiring，确保 committed Context/Memory/Config 与 gate/lease 行为经过生产 `resolve_session`。`SessionRunFixture` 只是拥有测试输入的 Builder/Object Mother；装配动作本身始终委托 production `RunFactory`。

- [ ] **Step 5: 实现 Derived fixture**

`tests/run_factory_support/derived_run.rs` 提供接受 production parent capability 的 Object Mother：

```rust
pub(crate) struct ParentRunFixture {
    pub(crate) context_factory: Arc<RuntimeContextFactory>,
    pub(crate) provider_factory: Arc<dyn ProviderFactory>,
    pub(crate) skill_catalog: Arc<dyn SkillCatalogPort>,
}

impl ParentRunFixture {
    pub(crate) fn create(
        &self,
        spec: RunSpec,
        session: SessionSnapshot,
        parent_run_id: RunId,
        parent_spec: RunSpec,
        parent_context: Arc<RuntimeContext>,
        parent_workspace: RuntimeWorkspaceAccess,
    ) -> Result<RunInstance, RunCreationError> {
        let request = RunCreationRequest::new(
            spec,
            session,
            Some(ParentRunFacts::new(parent_run_id, parent_spec)),
        )?;
        let bindings = ParentRunBindings::from_active_run(
            parent_context,
            parent_workspace,
        );
        RunFactory::for_parent(
            Arc::new(self.context_factory.with_derived_bindings(
                self.provider_factory.clone(),
                self.skill_catalog.clone(),
            )),
            bindings,
        )
        .create(request)
    }
}
```

真实 production 的 `ParentRunFrame` 已以 `Arc<RuntimeContext>` 保存 parent capability；fixture 复用同一所有权形状。若调用测试当前只有 `&RuntimeContext`，必须先让 parent fixture 返回/保存 `Arc<RuntimeContext>`，禁止通过 `RuntimeContext::clone` 伪装 parent identity，也不新增 `RunInstance` 拆包 API。

- [ ] **Step 6: 接入 `cfg(test)` 模块**

`run.rs` 增加：

```rust
#[cfg(test)]
#[path = "run/tests/run_factory_support.rs"]
pub(crate) mod run_factory_support;
```

`tests/run_factory_support.rs` 显式指向同目录下的 fixture 子文件：

```rust
#[path = "run_factory_support/derived_run.rs"]
pub(crate) mod derived_run;
#[path = "run_factory_support/fakes.rs"]
pub(crate) mod fakes;
#[path = "run_factory_support/session_run.rs"]
pub(crate) mod session_run;
```

- [ ] **Step 7: 运行 fixture 契约测试**

Run:

```bash
cargo test -p runtime test_fixture_uses_the_production_run_factory_chain -- --exact
```

Expected: PASS。

---

## Task 5：迁移 Session Run L2 装配测试

**Files:**
- Modify: `agent/features/runtime/src/application/run/context_factory_tests.rs`
- Test: `agent/features/runtime/src/application/run/context_factory_tests.rs`

- [ ] **Step 1: 删除测试 selector 断言并写生产结果断言**

删除直接调用：

```text
select_interaction
select_interaction_with_parent
select_hook
select_reasoning
```

改为通过 `SessionRunFixture::create(main_spec())` 断言：

```rust
let instance = fixture.create(main_spec()).expect("create session run");
assert!(instance.run().parent_id().is_none());
assert_eq!(instance.session().revision(), expected_revision);
assert!(Arc::ptr_eq(&instance.context().interaction(), &expected_interaction));
assert!(Arc::ptr_eq(&instance.context().reasoning(), &expected_reasoning));
instance.context().event_sink().try_send_event(marker_event());
assert_eq!(recording_event_sink.events(), vec![marker_event()]);
```

- [ ] **Step 2: 增加 Session 字段完整性失败测试**

覆盖 Context、Model、Tool、Event、Control：

```rust
assert!(Arc::ptr_eq(&context.context(), &committed_context));
assert!(Arc::ptr_eq(&context.memory(), &committed_memory));
assert!(Arc::ptr_eq(&context.provider(), &provider));
assert!(Arc::ptr_eq(&context.tool_catalog(), &services.tool_catalog));
assert!(Arc::ptr_eq(&context.tool_execution(), &services.tool_execution));
assert!(Arc::ptr_eq(&context.policy(), &services.policy));
assert!(Arc::ptr_eq(&context.hooks(), &services.hooks));
assert_eq!(context.usage().get(), None);
assert!(!context.input().is_sealed());
assert!(!context.cancel().token().is_cancelled());
```

实际 Rust 断言按 accessor 返回的 `Arc` 先绑定局部变量再用 `Arc::ptr_eq`，避免对临时值取引用；EventSink 通过 `RecordingEventSink` marker 事件验证委托身份。

- [ ] **Step 3: 增加独立 per-Run 资源测试**

从同一 fixture 创建两个 `RunInstance`，断言：

```rust
assert!(!Arc::ptr_eq(&first.context().reasoning(), &second.context().reasoning()));
first.context().input().push(marker_input());
assert!(second.context().input().with_lock(|buffer| buffer.is_empty()));
first.context().cancel().token().cancel();
assert!(!second.context().cancel().token().is_cancelled());
first.context().usage().update(17);
assert_eq!(second.context().usage().get(), None);
```

取消、usage、input 均互不污染；跨 Run 稳定 services 保持同 Arc。

- [ ] **Step 4: 增加 committed snapshot/gate 测试**

构造 request snapshot 后更新 Session wiring committed config；执行 `RunFactory::create`，断言最终 `RunInstance.session()` 与 `RuntimeContext.config()` 使用 gate 下绑定的 committed revision，且 request 外部对象不被回写。

- [ ] **Step 5: 运行 Session Run L2 测试**

Run:

```bash
cargo test -p runtime application::run::context_factory_tests -- --nocapture
```

Expected: PASS；不再存在任何 test-only selector 测试。

---

## Task 6：迁移 RuntimeContext 行为测试

**Files:**
- Modify: `agent/features/runtime/src/application/run/context_tests.rs`
- Test: `agent/features/runtime/src/application/run/context_tests.rs`

- [ ] **Step 1: 将 `make_context` 改为 production fixture**

替换为：

```rust
fn make_run_instance() -> RunInstance {
    SessionRunFixture::default()
        .create(RunSpec::main())
        .expect("create session run")
}
```

每个 Context 行为测试通过：

```rust
let instance = make_run_instance();
let context = instance.context();
```

- [ ] **Step 2: 将 identity 注入测试放入 fixture builder**

不再手填 `RunCapabilityBindings`。为 fixture builder 提供明确 service/session binding setter：

```rust
SessionRunFixtureBuilder::new()
    .with_context_port(context_port)
    .with_provider_binding(provider_binding)
    .with_interaction(interaction)
    .with_reasoning(reasoning)
    .with_event_sink(event_sink)
    .build()
```

builder 最终只构造 `SessionRunBindings` 并调用 `RunFactory`；不得构造 `RuntimeContext`。

- [ ] **Step 3: 保留纯局部 L1 测试**

`RunCancellationScope`、`RunUsageTracker`、`RunInputBufferHandle` 的纯局部测试继续直接构造这些值对象；不要为它们强制经过 RunFactory。

- [ ] **Step 4: 删除 direct-create 和 test token 引用**

Run:

```bash
rg -n 'RunCapabilityBindings|RuntimeContextFactory::create|\.create\(&.*bindings|new_for_test' \
  agent/features/runtime/src/application/run/context_tests.rs
```

Expected: 无匹配。

- [ ] **Step 5: 运行 Context L1/L2 测试**

Run:

```bash
cargo test -p runtime application::run::context_tests -- --nocapture
```

Expected: PASS，且测试持有完整 `RunInstance` 生命周期，session lease 不被提前释放。

---

## Task 7：迁移 Derived Run L2 装配测试

**Files:**
- Modify: `agent/features/runtime/src/application/run/derived/tests/runtime_context_derivation.rs`
- Modify: `agent/features/runtime/src/application/run/derived/tests/runtime_context_wiring.rs`
- Modify: `agent/features/runtime/src/application/run/derived/tests.rs`
- Test: same files

- [ ] **Step 1: 删除 parent Context 手工装配 helpers**

删除：

```text
assemble_parent_context
assemble_test_context
make_parent_context_with_factory
RunCapabilityBindings literals
```

统一改为 `SessionRunFixture` 创建 parent `RunInstance`。

- [ ] **Step 2: 先写 parent facts/bindings 成对失败测试**

通过 production `RunFactory` 验证 parent facts/bindings 必须成对：

```rust
let no_parent_request = RunCreationRequest::new(
    RunSpec::main(),
    session_snapshot.clone(),
    None,
)?;
let result = RunFactory::for_parent(context_factory.clone(), parent_bindings.clone())
    .create(no_parent_request);
assert!(matches!(result, Err(RunCreationError::ContextAssembly)));

let parent_request = RunCreationRequest::new(
    parent_spec.derive_sub("coder", timeout)?,
    session_snapshot,
    Some(parent_facts),
)?;
let result = RunFactory::for_session(context_factory, session_bindings)
    .create(parent_request);
assert!(matches!(result, Err(RunCreationError::ContextAssembly)));
```

第一例在 request 无 parent facts 时使用 Session 规格，避免 `RunSpec::validate_against` 提前截断；第二例使用真实 derived spec。

- [ ] **Step 3: 迁移 capability 收缩断言**

Derived `RunInstance` 必须断言：

```rust
assert_eq!(derived.run().parent_id(), Some(parent.run().id()));
assert!(!Arc::ptr_eq(&derived.context().context(), &parent.context().context()));
assert!(!Arc::ptr_eq(&derived.context().tool_catalog(), &parent.context().tool_catalog()));
assert!(!Arc::ptr_eq(&derived.context().interaction(), &parent.context().interaction()));
assert!(Arc::ptr_eq(&derived.context().policy(), &parent.context().policy()));
assert!(!Arc::ptr_eq(&derived.context().reasoning(), &parent.context().reasoning()));
```

并验证 hook 为受限 adapter、event sink 不向 parent 泄漏、cancel 为 child scope、skill-load identity/state 按设计继承。

- [ ] **Step 4: 保留 typed error matrix**

用 production fixture 覆盖：

```text
SubRoleNotFound
SubRoleDisabled
SubRoleNoModel
SubUnknownModel
SubProviderBuild
SubToolCatalog
CapabilityEscalation
ContextAssembly
```

不得通过测试 selector 人工返回错误。

- [ ] **Step 5: 运行 Derived L2 测试**

Run:

```bash
cargo test -p runtime application::run::derived::tests::runtime_context_derivation -- --nocapture
cargo test -p runtime application::run::derived::tests::runtime_context_wiring -- --nocapture
```

Expected: PASS。

---

## Task 8：迁移 client、pre-compact 与 Loop 测试 fixture

**Files:**
- Modify: `agent/features/runtime/src/application/client/from_args.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/pre_compact_trigger_tests.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs`
- Test: same modules

- [ ] **Step 1: 迁移 `from_args.rs` 测试 direct-create**

测试中的 `make_context`/`make_bindings` 改为 `SessionRunFixtureBuilder`；保留原行为断言，不改变 production bootstrap。

- [ ] **Step 2: 迁移 pre-compact trigger fixture**

把 `RuntimeContextFactory::create` 替换为：

```rust
let instance = fixture.create(RunSpec::main()).expect("create run");
let context = instance.context();
```

确保完整 instance 在异步测试期间存活。

- [ ] **Step 3: 决定并收口 `with_hooks`**

若 `with_hooks` 仅用于测试，则删除该方法，并让 `SessionRunFixtureBuilder::with_hook_port` 在构造 `RuntimeContextFactory::new` 前注入 Hook。若 production/Composition 有真实消费者，则保留为返回新 immutable factory 的构造 API并补 owner 说明；不得保留“测试专用 Context 创建”能力。

- [ ] **Step 4: 迁移 Loop Hook tests**

把：

```rust
shell.runtime_context_factory.with_hooks(test_hook_port())
```

改为测试 shell/builder 创建时提供 Hook service；不改变 Stop Hook 时序或终态断言。

- [ ] **Step 5: 运行定向测试**

Run:

```bash
cargo test -p runtime application::client::from_args -- --nocapture
cargo test -p runtime pre_compact_trigger -- --nocapture
cargo test -p runtime application::loop_engine::chat::loop_runner_tests -- --nocapture
```

Expected: PASS。

---

## Task 9：物理删除并行装配入口

**Files:**
- Modify: `agent/features/runtime/src/application/run/context_factory.rs`
- Modify: `agent/features/runtime/src/application/run/context.rs`
- Modify: `agent/features/runtime/src/application/run/context_factory_tests.rs`
- Test: Runtime Run tests

- [ ] **Step 1: 删除 `RuntimeContextFactory` test-only impl 中的创建算法**

删除：

```text
create
select_interaction
select_interaction_with_parent
select_hook
select_reasoning
```

若 Task 8 已迁移完 Hook fixture，同时删除 `with_hooks`；否则只保留不创建 Context、不选择 capability 的 immutable service replacement。

- [ ] **Step 2: 删除测试 token**

从 `context.rs` 删除：

```rust
#[cfg(test)]
impl RuntimeContextAssemblyToken {
    pub(crate) fn new_for_test() -> Self { Self(()) }
}
```

并将 token 文档改为只允许 `RuntimeContextFactory` 生产装配调用。

- [ ] **Step 3: 修正过期注释**

将本次触及代码中的 `RuntimeContextFactory::create` 改为：

```text
RunFactory::create
RuntimeContextFactory::prepare（仅 RunFactory 内部）
```

删除本次触及代码/注释中的外部追踪编号。

- [ ] **Step 4: 搜索确认物理清零**

Run:

```bash
rg -n 'RuntimeContextFactory::create|new_for_test|select_interaction_with_parent|pub fn select_interaction|pub fn select_hook|pub fn select_reasoning' \
  agent/features/runtime/src
rg -n 'RuntimeContext::new\(' agent/features/runtime/src
rg -n '\.prepare\(' agent/features/runtime/src
rg -n 'RunInstance::new\(' agent/features/runtime/src
```

Expected:

- 第一组无匹配；
- `RuntimeContext::new` 只有 `context_factory.rs` 一处；
- `.prepare` 只有 `factory.rs` 一处；
- `RunInstance::new` 只有 `factory.rs` 一处。

- [ ] **Step 5: 运行 Run 模块测试与 Guard**

Run:

```bash
cargo test -p runtime application::run -- --nocapture
bash .agents/hooks/check-runtime-capability-assembly-tests.sh
bash .agents/hooks/check-runtime-capability-assembly.sh
```

Expected: 全部 PASS。

---

## Task 10：补 Composition L3 单实例注入契约

**Files:**
- Modify: `agent/composition/src/runtime_tests.rs`
- Modify: `agent/composition/tests/main_session_wiring.rs`
- Modify: `agent/composition/src/runtime.rs`
- Test: same files

- [ ] **Step 1: 在 Composition 内写字段完整性契约 helper**

定义一次断言 helper，验证 Runtime assembly 对 Context、Model、Tool、Event、Control 的必要输入全部来自一个 object graph；不要为 Main/Derived 复制两套断言。

- [ ] **Step 2: 写同一 factory Arc 注入测试**

通过 Composition 内可见的 typed assembly 测试，断言：

```rust
assert!(Arc::ptr_eq(
    bootstrap.runtime_context_factory(),
    agent_runner.runtime_context_factory(),
));
```

若现有 assembly 无只读 accessor，优先在 Composition-local assembly 测试构造函数中保留 clone 进行断言；不要向 Runtime 公共 façade 新增 test accessor。

- [ ] **Step 3: 验证 derived bindings 进入同一 factory 值链**

构造 provider factory 与 skill catalog spy，确认 agent runner 与 bootstrap 持有同一基础 factory Arc；Derived 创建通过该 factory 的 production `with_derived_bindings` immutable 派生值进入唯一 `prepare` 算法，而不是第二装配算法。再通过最终 Derived `RunInstance` 验证 tool/policy/hook/reflection/task 与 Main services 的共享/收缩规则。

- [ ] **Step 4: 清理 Composition 过期注释**

删除本次触及行的外部追踪编号，改为稳定的所有权描述。

- [ ] **Step 5: 运行 Composition L3 测试**

Run:

```bash
cargo test -p composition --lib runtime -- --nocapture
cargo test -p composition --test main_session_wiring -- --nocapture
```

Expected: PASS，且 Runtime crate root 无新增导出。

---

## Task 11：补 Main/Derived L4 单 Loop 场景

**Files:**
- Create: `agent/features/runtime/src/application/run/scenario_tests.rs`
- Create: `agent/features/runtime/src/application/run/scenario_tests/main_run.rs`
- Create: `agent/features/runtime/src/application/run/scenario_tests/derived_run.rs`
- Modify: `agent/features/runtime/src/application/run.rs`
- Test: scenario files

- [ ] **Step 1: 写 Main 场景失败测试**

场景必须实际执行：

```text
SessionRunFixture
→ RunFactory::create
→ RunInstance.initialize
→ RunLauncher::launch
→ single Loop terminal
```

用 scripted provider 返回简单 EndTurn；断言输入通过 `RunInputBuffer`、模型读取装配 Context、终态来自同一 `RunInstance`。

- [ ] **Step 2: 运行 Main 场景确认失败**

Run:

```bash
cargo test -p runtime main_run_uses_single_factory_launcher_and_loop -- --exact --nocapture
```

Expected: FAIL，直到场景 harness 完成。

- [ ] **Step 3: 实现 Main 场景 harness**

只复用 production `RunLauncher::launch` 与已有窄 observer/fake；不新增第二 launcher 或直接调用 Loop 内部阶段。

- [ ] **Step 4: 写 Derived 场景失败测试**

场景必须实际执行：

```text
production parent capability frame
→ ParentRunFacts + ParentRunBindings
→ same RuntimeContextFactory
→ RunFactory::create
→ restricted Derived RunInstance
→ RunLauncher::launch
→ same Loop terminal
```

断言 parent relation、capability 不扩权、workspace/event/interaction/sibling 隔离。

- [ ] **Step 5: 实现 Derived 场景 harness**

用同一 scripted provider/factory 风格；Derived 与 Main 只在 request/topology/bindings 上不同，启动函数必须相同。场景中的 parent capability 必须先由 Session fixture 经 production RunFactory 创建，再以 production `ParentRunFrame` 的 `Arc<RuntimeContext> + workspace + RunId + RunSpec` 形状提供给 Derived fixture。

- [ ] **Step 6: 运行 L4 场景**

Run:

```bash
cargo test -p runtime application::run::scenario_tests -- --nocapture
```

Expected: Main/Derived 场景全部 PASS。

---

## Task 12：同步 Guard、Runtime 设计与证据矩阵

**Files:**
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
- Modify: `docs/design/03-engineering/04-testing-and-coverage.md`
- Modify: `docs/design/02-modules/runtime/07-runtime-ownership-and-assembly.md`
- Test: docs/Guard checks

- [x] **Step 1: 修正 Runtime 唯一创建链**

文档统一使用：

```text
RunCreationRequest
+ SessionRunBindings / ParentRunBindings
→ RunFactory::create
→ RuntimeContextFactory::prepare
→ RuntimeContext::new
→ RunInstance
→ RunLauncher::launch
→ single Loop Engine
```

删除把 `RuntimeContextFactory::create` 描述为外部入口的伪代码。

- [x] **Step 2: 更新 Guard registry**

记录新增不变量及 probe：

```text
RuntimeContext::new only in context_factory.rs
RuntimeContextFactory::prepare only called by factory.rs
RunInstance::new only in factory.rs
no cfg(test) RuntimeContext creator
RunCreationRequest/SessionSnapshot/ParentRunFacts are pure values
Main/Derived both create and launch complete RunInstance
```

- [x] **Step 3: 登记 L0–L4 矩阵**

在测试治理文档新增稳定领域标题，不引用外部追踪号：

| 层级 | 证据 |
|---|---|
| L0 | Runtime capability guard + deliberate probes |
| L1 | RunSpec ceiling、RunCreationRequest、SessionSnapshot |
| L2 | Session/Derived RunFactory production-chain tests |
| L3 | Composition same-factory and field completeness contracts |
| L4 | Main/Derived same-launcher/same-Loop scenarios |

- [x] **Step 4: 扫描稳定文档约束**

Run:

```bash
rg -n 'RuntimeContextFactory::create|#[0-9]+|PR #[0-9]+|Issue #[0-9]+' \
  docs/design/02-modules/runtime/07-runtime-ownership-and-assembly.md \
  docs/design/03-engineering/01-architecture-guards.md
```

Expected: 本次修改段落无过期入口或外部追踪编号；若文件历史段落仍存在编号，按仓库既定规则在本次触及文件中一并清理为稳定术语。

- [x] **Step 5: 运行文档与 Guard 验证**

Run:

```bash
git diff --check
bash .agents/hooks/check-runtime-capability-assembly-tests.sh
bash .agents/hooks/check-runtime-capability-assembly.sh
```

Expected: PASS。

---

## Task 13：执行定向回归与死代码检查

**Files:**
- Verify only

- [x] **Step 1: 格式检查**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS。若失败，只运行 `cargo fmt --all`，不得手动调整格式。

- [x] **Step 2: Runtime L1/L2/L4 回归**

Run:

```bash
cargo test -p runtime --lib
```

Expected: 全部 PASS；记录测试总数和首次结果。

- [x] **Step 3: Composition L3 回归**

Run:

```bash
cargo test -p composition --tests
```

Expected: 全部 PASS。

- [x] **Step 4: 生产可达性**

Run:

```bash
cargo run -p xtask -- production-reachability .
```

Expected: PASS；确认删除 test-only creator 后没有仅测试托活的生产 API。

- [x] **Step 5: 搜索退役路径与第二算法**

Run:

```bash
rg -n 'RuntimeContextFactory::create|new_for_test|RunCapabilityBindings' \
  agent/features/runtime/src/application/run \
  agent/features/runtime/src/application/client \
  agent/features/runtime/src/application/loop_engine/chat
rg -n 'RuntimeContext::new\(' agent/features/runtime/src
rg -n '\.prepare\(' agent/features/runtime/src
rg -n 'RunInstance::new\(' agent/features/runtime/src
```

Expected:

- test-only create/new_for_test/direct test binding 无匹配；
- 构造/prepare/new 各只有批准位置；
- `RunCapabilityBindings` 只可作为 `context_factory.rs → RuntimeContext::new` 的内部局部组装类型，不流入 fixture/caller。

---

## Task 14：执行完整门禁

**Files:**
- Verify only

- [x] **Step 1: Workspace all-target clippy**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS；记录首次失败，不用重跑成功覆盖。

- [x] **Step 2: 完整架构守卫**

Run:

```bash
.agents/hooks/check-architecture-guards.sh --full
```

Expected: PASS。

- [x] **Step 3: Workspace 测试**

首次执行发现 `application::loop_engine::chat::snapshot_registry::tests::test_no_change_detected` 在 workspace 并行运行时因临时目录竞争报 `NotFound`；该测试不在本次改动链路中，随后精确单测通过。按门禁约定保留首次失败证据，不宣称 workspace 全绿。

Run:

```bash
cargo test --workspace
```

Expected: PASS。若存在与本改动无关的已知 blocker，保存原始命令、退出码、失败测试和定向分类证据，不宣称 workspace 全绿。

- [x] **Step 4: 最终状态审计**

Run:

```bash
git status --short --branch
git diff --stat
git diff --check
git diff --name-status
```

Expected: 只有计划内文件变化，无临时 probe、生成物或 fixture 残留。

- [x] **Step 5: 完成 Task list**

更新当前 Task list：每项必须有可验证结果；有 blocker 的任务保持阻塞而非错误标记 completed。

---

## Task 15：提交前自审与执行交接

**Files:**
- Read: all changed files
- Verify: plan/spec coverage

- [x] **Step 1: 逐条对照设计完成定义**

确认：

```text
- no test-only RuntimeContext creation algorithm
- all migrated tests obtain RunInstance via RunFactory::create
- one prepare caller
- one RuntimeContext constructor site
- one RunInstance constructor site
- same factory / launcher / Loop for Main and Derived
- no capability escalation
- L0 deliberate probes and L1-L4 evidence present
```

- [x] **Step 2: 检查计划外改动**

Run:

```bash
git diff --name-only
```

Expected: 每个文件都可映射到本计划的文件职责地图；否则回滚计划外改动或先向用户说明并获得扩展授权。

- [x] **Step 3: 准备提交摘要但不自动提交**

输出：改动摘要、验证证据、首次失败记录、剩余风险。只有用户明确要求提交/推送时才执行 Git 提交与推送。

---

## 自审结果

### Spec 覆盖

- 唯一创建链：Tasks 4–9。
- Guard 正向结构与防复活：Tasks 2–3、9、12。
- L1：Task 5 中 request/snapshot/ceiling 保留，Derived error matrix 在 Task 7。
- L2 Session/Derived 字段完整性：Tasks 5–7。
- L3 Composition 同一 factory 与无覆写：Task 10。
- L4 Main/Derived 同 launcher/Loop：Task 11。
- 文档与证据矩阵：Task 12。
- 完整门禁、死代码和首次失败保留：Tasks 13–15。

### Placeholder 扫描

计划中的所有实施步骤都给出了确定文件、动作、命令和期望结果，不含待定实现占位。

### 类型一致性

- 外部创建入口始终为 `RunFactory::create(RunCreationRequest) -> Result<RunInstance, RunCreationError>`。
- Context 内部装配始终为 `RuntimeContextFactory::prepare(&RunCreationRequest, &RunCreationBindings)`。
- Session/Derived live 输入分别为 `SessionRunBindings` / `ParentRunBindings`。
- 启动入口始终接收完整 mutable `RunInstance`。
