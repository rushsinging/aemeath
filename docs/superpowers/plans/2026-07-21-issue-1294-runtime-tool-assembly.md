# #1294 Runtime Tool/Skill 装配权迁移实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Tool Catalog/Execution、Skill materializer、Tool Result materializer 与 ActiveRunRegistry 的生产构造权从 Runtime 迁至 Composition，保持 Runtime 的编排与现有行为不变。

**Architecture:** Composition 在 runtime bootstrap 中一次创建 Tools private backing 的双端口、同一 Skill materializer、Tool Result AtomicBlob store/materializer 与 ActiveRunRegistry，并作为 `RuntimeBootstrapDependencies` 注入。Runtime 只解构和转发这些已装配资源；Main 与 Sub runner 复用同一 Skill materializer、Tool Result materializer 和 ActiveRunRegistry。Tool Result policy 由 Composition 从已完成 Config wiring 的 `ConfigSnapshot` 读取后传入其 local factory；Runtime 不再选择任何 Tool Result materialization policy。

**Tech Stack:** Rust、Tokio、Tool Catalog/Execution PL、SkillMaterializationPort、AtomicBlobPort、Cargo tests、shell architecture guards。

---

## 文件结构

| 文件 | 职责 / 改动 |
|---|---|
| `agent/features/runtime/src/application/client/from_args.rs` | 扩展 dependencies，移除 Tools/Skill/Storage/registry factory，消费 injected resources。 |
| `agent/features/runtime/src/application/startup/runtime_support.rs` | `build_agent_runner` 显式接收并复用 injected Skill materializer。 |
| `agent/features/runtime/src/application/active_run.rs` | 将仅供 Composition 构造所需的 registry 类型以受控 public API 暴露，保留 Runtime domain port。 |
| `agent/features/runtime/src/application/tool_result_materialization.rs` | 为 Composition 提供最小化 policy/materializer 构造 API。 |
| `agent/features/runtime/src/lib.rs` | 发布 Composition 所需的窄 runtime assembly 类型，不暴露 Runtime resources。 |
| `agent/composition/src/runtime.rs` | 唯一装配 Tools、Skill、Tool Result 与 active-run，并注入 dependencies。 |
| `agent/composition/tests/main_session_wiring.rs` | 断言 Composition 注入资源后 Main bootstrap 可完成，且相同实例被转发。 |
| `agent/features/runtime/tests/bootstrap_dependencies.rs` | 断言 injected ports/materializers/registry identity 保持。 |
| `.agents/hooks/check-runtime-tool-assembly-ownership.sh` | 禁止 Runtime production factory 和 concrete Tool Result storage 构造，要求 Composition 装配。 |
| `.agents/hooks/check-runtime-tool-assembly-ownership-tests.sh` | 两类故意违规均稳定 exit 2。 |
| `.agents/hooks/check-architecture-guards.sh`、`.agents/architecture-guard-registry.json` | 注册 Guard，零 allow/exclude/skip 净增。 |
| `docs/design/02-modules/{runtime/06-ports-and-adapters.md,tools/02-ports-and-lifecycle.md}` | 将 production assembly 事实修正为 Composition。 |
| `docs/design/01-system/03-context-map.md`、`docs/design/03-engineering/{01-architecture-guards.md,03-migration-governance.md}` | 更新跨域构造权、Guard 与迁移证据。 |

## Task 1：建立失败的依赖注入契约测试

**Files:**
- Modify: `agent/features/runtime/tests/bootstrap_dependencies.rs`
- Modify: `agent/composition/tests/main_session_wiring.rs`

- [x] **Step 1: 写 Runtime dependency identity 测试**

在现有 `bootstrap_dependencies_preserve_injected_task_views` fixture 中创建 Tools test harness、Skill materializer、test Tool Result materializer 和 ActiveRunRegistry；调用扩展后的 `RuntimeBootstrapDependencies::new`，断言每个 accessor 返回同一 `Arc` 实例。

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test -p runtime --test bootstrap_dependencies injected -- --nocapture`

Expected: 编译失败，原因是 dependencies 尚未接收这些资源。

- [x] **Step 3: 写 Composition bootstrap 转发场景测试**

在 `agent/composition/tests/main_session_wiring.rs` 使用真实 Composition bootstrap，断言 Runtime 不需要自行构造 Tool Result backing；场景只验证依赖装配成功，不执行真实 Provider 调用。

## Task 2：发布最小 Runtime assembly API 并注入 dependencies

**Files:**
- Modify: `agent/features/runtime/src/application/client/from_args.rs`
- Modify: `agent/features/runtime/src/application/active_run.rs`
- Modify: `agent/features/runtime/src/application/tool_result_materialization.rs`
- Modify: `agent/features/runtime/src/lib.rs`
- Test: Task 1 tests

- [x] **Step 1: 扩展 `RuntimeBootstrapDependencies`**

新增并发布以下资源：
```rust
tool_catalog: Arc<dyn tools::ToolCatalogPort>,
tool_execution: Arc<dyn tools::ToolExecutionPort>,
tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
tool_result_materializer: Arc<ToolResultMaterializer>,
active_run: Arc<ActiveRunRegistry>,
```

构造器和 clone accessor 逐项接收/返回；不得在 Runtime 公开 Tools private backing、ToolRegistry 或 Storage port。

- [x] **Step 2: 使 Composition 可构造 Runtime-owned adapters**

将 `ActiveRunRegistry` 和 `ToolResultMaterializer` 通过 `runtime::assembly`（或等价窄 module）发布给 Composition：只暴露构造与 Runtime 所需 domain-port 形状；不发布内部 map、Tool Result blob key 或 `RuntimeResources`。`AtomicBlobToolResultStore` 的 concrete adapter 留在 Runtime crate，但由 Composition 调用其 constructor。保持 cancellation terminal claim、Tool Result key/threshold/preview/idempotence 语义不变。

- [x] **Step 3: 修改 `from_args_with_workspace`**

解构 injected resources，删除：
```rust
tools::composition::wire_builtin_catalog_execution(...)
tools::composition::wire_skills()
storage::FileSystemBlobAdapter::new(...)
AtomicBlobToolResultStore::new(...)
ActiveRunRegistry::default()
```

Runtime 只把 injected materializer/registry 分发给 Main resources 与 Sub runner；Tool Result policy 的 snapshot 解读及 concrete store/materializer 构造一并移至 Composition helper。

- [x] **Step 4: 运行 Task 1 测试确认通过**

Run: `cargo test -p runtime --test bootstrap_dependencies -- --nocapture`

Expected: PASS，所有 injected identity 断言成立。

## Task 3：Composition 唯一装配 Tool/Skill/Tool Result/active-run

**Files:**
- Modify: `agent/composition/src/runtime.rs`
- Modify: `agent/composition/tests/main_session_wiring.rs`

- [x] **Step 1: 写失败测试**

在 `agent/composition/tests/main_session_wiring.rs` 的现有 production bootstrap fixture 中，先断言真实 Composition bootstrap 成功装配 Tools catalog/execution/binding、一个 Skill wiring、Tool Result materializer 与 active-run；新增 test-only Composition accessor 或受控 assembly result，使测试能用 `Arc::ptr_eq` 证明 Context main factory 与 Runtime dependencies 获得同一 Skill materializer。不得执行真实 Provider 调用。

- [x] **Step 2: 增加 Composition-private assembly helper**

在 `runtime.rs` 创建 helper：
```rust
fn wire_runtime_tool_assembly(
    task_access: Arc<dyn task::TaskAccess>,
    memory_source: Arc<dyn tools::MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
    snapshot: &ConfigSnapshot,
) -> Result<RuntimeToolAssembly, SdkError>
```

其内部唯一调用 `tools::composition::wire_builtin_catalog_execution`、`wire_skills`，从 `snapshot.tool_result_policy()` 建立 Runtime-owned policy/value，再构造 Tool Result filesystem blob/store/materializer 和 ActiveRunRegistry。错误保持 `SdkError::Init`。

- [x] **Step 3: 同一 Skill materializer 分发给 Context 与 Runtime**

先创建 `skill_wiring`，将 `skill_wiring.materializer()` 传给 `ProductionMainContextFactory::with_skill_supplier`，并将 clone 注入 Runtime dependencies；禁止保留 `wire_skill_materialization()` 的第二次生产调用。

- [x] **Step 4: 注入 dependencies 并运行场景测试**

Run: `cargo test -p composition --test main_session_wiring -- --nocapture`

Expected: PASS；Composition 是唯一 production factory，Runtime 只接收 ports/materializers/registry。

## Task 4：让 Sub runner 复用 Main 注入资源

**Files:**
- Modify: `agent/features/runtime/src/application/startup/runtime_support.rs`
- Modify: 受影响的 Runtime unit tests 与 runner fixtures

- [x] **Step 1: 写失败测试**

为 `build_agent_runner` 添加 identity test：传入 marker Skill materializer，断言 runner 使用该 materializer，不再内部 `wire_skill_materialization()`。

- [x] **Step 2: 改造 runner 构造签名**

新增：
```rust
skill_materializer: Arc<dyn tools::SkillMaterializationPort>
```

并直接存入 runner。更新所有生产和测试调用点；测试 fixture 可使用 existing Tools test harness，不得新增全局 fixture。

- [x] **Step 3: 运行定向 Runtime 测试**

Run:
```bash
cargo test -p runtime build_agent_runner -- --nocapture
cargo test -p runtime --test bootstrap_dependencies -- --nocapture
```

Expected: PASS；Main/Sub 未再创建第二个 Skill materializer。

## Task 5：构造权 Guard、文档和门禁回填

**Files:**
- Create: `.agents/hooks/check-runtime-tool-assembly-ownership.sh`
- Create: `.agents/hooks/check-runtime-tool-assembly-ownership-tests.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/architecture-guard-registry.json`
- Modify: design docs listed above
- Modify: Issue #1294 body/comment

- [x] **Step 1: 写 Guard 负例**

Sanity script 临时注入两种违规并断言 exit 2：
1. `from_args.rs` 新增 `tools::composition::wire_builtin_catalog_execution`；
2. `from_args.rs` 新增 `FileSystemBlobAdapter::new` 或 `ActiveRunRegistry::default`。

恢复后单 guard 必须通过。

- [x] **Step 2: 实现并注册 Guard**

Guard 要求 Runtime production source 零 Tools composition factory、零 Tool Result concrete storage、零 ActiveRunRegistry factory；Composition `runtime.rs` 必须装配并传入全部依赖。禁止新增 allow/exclude/skip。

- [x] **Step 3: 更新文档与 #1294 差异表**

明确 Composition 是唯一 production assembly root；Runtime 只持 PL/port/materializer/registry。记录 Tool Result key/threshold/preview/idempotence、Tool Scope/Profile、Hook/MCP lifecycle 均未变化。

- [ ] **Step 4: 执行完整验证**

Run:
```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash .agents/hooks/check-architecture-guards.sh --full
.agents/hooks/check-runtime-tool-assembly-ownership-tests.sh
git diff --check
```

Expected: 全部 PASS。

- [ ] **Step 5: 检查退役路径并提交**

Run:
```bash
grep -RInE 'tools::composition::wire_(builtin_catalog_execution|skills|skill_materialization)|FileSystemBlobAdapter::new|ActiveRunRegistry::default' agent/features/runtime/src --include='*.rs'
```

Expected: production `from_args.rs` / `runtime_support.rs` 无 factory；仅 Runtime 自身 test fixture 可以构造 registry。

Commit:
```bash
git add agent/features/runtime agent/composition .agents docs/design
git commit -m "refactor(runtime): #1294 迁移 Tool 装配权"
```

Commit body MUST include `Refs #1294`, `Refs #950`, and the repository-standard AI co-author trailer.
