# RuntimeContext 单一装配链与分层守卫设计

> 状态：已批准，待实施计划
> 范围：RuntimeContext / RunFactory / 单一 Loop 装配的结构守卫与 L0–L4 验证

## 1. 背景与问题

Runtime 生产链已经使用：

```text
RunCreationRequest
  + SessionRunBindings / ParentRunBindings
  → RunFactory::create
  → RuntimeContextFactory::prepare
  → RuntimeContext::new
  → RunInstance
  → RunLauncher::launch
  → 单一 Loop Engine
```

但 `RuntimeContextFactory` 仍保留 `#[cfg(test)] create()`，允许测试直接传入 `RunCapabilityBindings` 并单独实现 interaction、hook 与 skill-load 选择。该入口与生产 `prepare()` 不是同一算法，造成以下风险：

1. 测试可绕过 `RunCreationRequest`、Session/Parent live bindings、`RunFactory` 与 `RunInstance`；
2. 测试通过不证明生产装配字段完整，也不能证明 Main 与 Derived 使用同一创建链；
3. interaction、hook、reasoning 等能力选择在测试 helper 中重复实现，生产算法变化后测试仍可能绿；
4. Guard 当前会主动移除 `#[cfg(test)]` item 再扫描，无法阻止测试侧第二装配入口；
5. `select_interaction`、`select_hook`、`select_reasoning` 等测试 selector 直接重复读取 `RunSpec`，不覆盖真正的 capability adapter 绑定结果。

本设计的目标是删除测试并行装配算法，让生产与测试只能经过同一 Run 创建链，并用 L0–L4 相邻证据证明结构、策略、装配和最终场景。

## 2. 目标与非目标

### 2.1 目标

1. `RuntimeContext` 只由 `RuntimeContextFactory` 的单一生产算法构造；测试不得拥有第二算法。
2. Main、Derived 及测试均通过 `RunFactory::create(RunCreationRequest)` 取得完整 `RunInstance`。
3. `RunCreationRequest` 保持纯值；Session/Parent live capability 只通过 crate-private typed bindings 进入 `RunFactory`。
4. `RunFactory` 是 `Run + RunExecutionState + RuntimeContext + SessionSnapshot + Workspace` 的唯一聚合创建入口。
5. Main 与 Derived 使用相同 `RuntimeContextFactory`、`RunFactory::create`、`RunLauncher::launch` 和 Loop Engine。
6. Derived capability 相对 parent 只能收缩或平移，不能扩权。
7. 每条新增结构规则都有单 Guard 与总编排 `exit 2` 的故意违规证明。
8. Context、Model、Tool、Event、Control 每个相邻边界都有 L2/L3 证据，并由 L4 Main/Derived 场景证明最终组合。

### 2.2 非目标

1. 不改变 Run、Loop、Tool、Hook、Interaction 的业务语义。
2. 不新增公开测试 Harness，不扩大 Runtime crate-root API。
3. 不重写所有历史测试；只迁移直接依赖测试装配旁路及本设计所需的相邻契约测试。
4. 不把 Composition 的具体 adapter 创建移动到 Runtime。
5. 不新增第二种 launcher、第二种 RuntimeContext factory 或新的能力参数袋。
6. 不用字符串扫描替代可以由 Rust 类型与行为测试证明的业务语义。

## 3. 方案选择

### 3.1 方案 A：测试改调 `RuntimeContextFactory::prepare`

优点是改动较小。缺点是测试仍直接消费拆散的 `RuntimeContext + SessionSnapshot + Workspace`，绕过 `RunFactory`、`RunInstance` 和聚合不变量，无法证明完整生产创建链。

### 3.2 方案 B：测试统一经 `RunFactory::create`，删除测试装配旁路

优点：

- 生产与测试只有一套装配算法；
- 每个测试拿到完整 `RunInstance`，可验证 Context、Session、Workspace、parent relation 与执行聚合；
- capability 选择由真正的 Session/Parent bindings 驱动；
- Guard 可机械禁止第二入口和直接 RuntimeContext 构造；
- 能与 Main/Derived L4 场景形成完整证据链。

成本是需要迁移现有直接调用 `RuntimeContextFactory::create()` 的测试 fixture，并补齐 typed Session/Parent binding fixture。

### 3.3 方案 C：新增公开测试 Harness

优点是测试书写简洁。缺点是扩大稳定 API，并可能形成另一个高级入口和隐藏装配算法。本设计不采用。

### 3.4 决策

采用方案 B。测试 fixture 可以复用，但 fixture 的最终动作必须调用生产 `RunFactory::create()`；fixture 不得自行构造 `RuntimeContext` 或复制 capability selector。

## 4. 目标架构

### 4.1 唯一创建链

```text
Composition Root
  → RuntimeContextFactory::new / with_derived_bindings
  → Runtime bootstrap 注入同一 Arc<RuntimeContextFactory>

Session source
  → SessionState::snapshot_for_run
  → SessionRunBindings
  → RunFactory::for_session
  → RunFactory::create(RunCreationRequest)

Derived source
  → ParentRunFacts + ParentRunBindings
  → RunFactory::for_parent
  → RunFactory::create(RunCreationRequest)

RunFactory::create
  → RuntimeContextFactory::prepare（唯一 crate-private context 装配算法）
  → RuntimeContext::new（仅 context_factory.rs 可调用）
  → RunInstance::new（仅 factory.rs 可调用）
  → RunLauncher::launch(&mut RunInstance)
  → 单一 Loop Engine
```

`RuntimeContextFactory::prepare()` 保持 `pub(crate)` 仅供 `RunFactory` 使用；除 `RunFactory` 外，Runtime 生产代码和测试都不得直接调用它。

### 4.2 类型责任

| 类型 | 责任 | 禁止内容 |
|---|---|---|
| `RunCreationRequest` | RunSpec、SessionSnapshot、ParentRunFacts 纯值输入 | Arc、Port、RuntimeContext、Workspace live handle |
| `SessionRunBindings` | Main Session 的 typed live capability view | Run/Execution 创建算法 |
| `ParentRunBindings` | Derived 的父 Context 与 Workspace capability view | capability 升权与完整服务集合 |
| `RuntimeContextFactory` | 按 request + binding 选择并冻结 per-Run capability | 创建 Run、Execution 或启动 Loop |
| `RunFactory` | 协调 Context、Session、Workspace、Run 与 Execution 为 `RunInstance` | 模型/工具/Loop 业务执行 |
| `RunInstance` | 完整 Run 聚合 | public 拆包入口 |
| `RunLauncher` | 只启动完整 mutable `RunInstance` | 接收拆散的 Run/Execution/Context |

### 4.3 测试 fixture 边界

测试 fixture 位于 Runtime application/run owning layer 的显式 `tests/` 目录，并受 `cfg(test)` 约束。该物理边界让生产 Guard 可以机械排除 fixture，而不会把 Runtime 测试所需的 Context/Task/Config 装配误判为生产旁路。fixture 可提供：

- 固定 `SessionState` 与 `SessionSnapshot`；
- `SessionRunBindings` 的 fake wiring/provider/interaction/reasoning/event sink；
- `ParentRunBindings` 的 parent `RunInstance` 与 isolated workspace；
- 固定的 `RuntimeContextFactory` services；
- `create_session_run(spec)` 与 `create_derived_run(spec, parent)` 之类的测试动作。

这些测试动作必须内部调用 `RunFactory::for_session/for_parent(...).create(request)`。fixture 不得：

- 调用 `RuntimeContext::new`；
- 调用 `RuntimeContextFactory::prepare`；
- 复刻 interaction/hook/reasoning selector；
- 接收 `RunCapabilityBindings` 作为万能参数袋；
- 绕过 `RunCreationRequest::new` 的 parent capability ceiling 校验。

## 5. 生产代码收口

### 5.1 删除并行入口

从 `RuntimeContextFactory` 的 `#[cfg(test)] impl` 删除：

- `create`；
- `select_interaction`；
- `select_interaction_with_parent`；
- `select_hook`；
- `select_reasoning`。

`with_hooks` 只负责替换 factory service，仍可作为 test-only fixture 构造工具保留；它不能创建 Context，也不能选择 capability。若迁移后可由 fixture 直接构造 factory，则进一步删除 `with_hooks`。

### 5.2 收紧可见性与唯一调用者

- `RuntimeContextFactory::prepare` 只允许 `RunFactory::create` 调用；
- `RuntimeContext::new` 只允许 `context_factory.rs` 调用；
- `RunInstance::new` 只允许 `factory.rs` 调用；
- Main 与 Derived caller 只创建 typed bindings/request 并调用 `RunFactory::create`；
- `RunLauncher::launch` 只接收完整 mutable `RunInstance`。

### 5.3 清理过期说明

代码和设计文档中的 `RuntimeContextFactory::create()` 描述必须改为：

- 外部 Run 创建入口：`RunFactory::create()`；
- Context 内部装配入口：`RuntimeContextFactory::prepare()`，仅供 `RunFactory` 使用。

代码、注释与设计文档不引用外部追踪编号。

## 6. Guard 设计（L0）

### 6.1 正向结构约束

`check-runtime-capability-assembly.sh` 增加或收紧以下规则：

1. `RuntimeContext::new()` 只出现在 `application/run/context_factory.rs`。
2. `RuntimeContextAssemblyToken::new()` 只出现在 `context_factory.rs`。
3. `RuntimeContextFactory::prepare()` 只由 `application/run/factory.rs` 调用。
4. `RunInstance::new()` 只由 `application/run/factory.rs` 调用。
5. `RunFactory::create()` 只接收 `RunCreationRequest` 并返回 `RunInstance`。
6. Main caller 必须调用 `RunFactory::create()` 和 `RunLauncher::launch()`。
7. Derived setup 必须调用 `RunFactory::create()`，Derived launcher 必须把完整 `RunInstance` 交给统一 launcher。
8. `RunCreationRequest`、`SessionSnapshot`、`ParentRunFacts` 只含纯值。
9. Runtime crate-root 只导出批准的精确 façade；不导出 `RunFactory`、bindings 或 `RunInstance`。
10. Composition 的生产装配必须构造并注入 `RuntimeContextFactory`；Runtime application 不得构造供应 BC 的具体 adapter/object graph。

### 6.2 防复活约束

保留少量高价值、与命名无关或难以换名绕过的规则：

- 禁止任一 trait/supertrait/type alias 聚合多个 Loop capability category；
- 禁止任一生产或测试类型同时实现多个 Loop capability category；
- 禁止动态 capability locator；
- 禁止公开 `RunInstance` 拆包；
- 禁止测试文件直接调用 `RuntimeContextFactory::prepare` 或 `RuntimeContext::new`；
- 禁止任何 `#[cfg(test)]` 的 RuntimeContext 创建方法，其返回类型为 `RuntimeContext` 或内部调用 `RuntimeContext::new`；
- 禁止 `RunCapabilityBindings` 从 test fixture 流入 Context 构造入口。

Guard 优先检查结构和允许调用点，不依赖某个已退役名字的黑名单。旧名字黑名单只作为额外防复活，不作为唯一证明。

### 6.3 故意违规证明

`check-runtime-capability-assembly-tests.sh` 为每条新增规则创建临时违规，要求：

- 单 Guard 返回 `exit 2`；
- 诊断包含稳定、可定位的不变量描述；
- 恢复 baseline 后 Guard 再次通过。

至少包含：

1. 在测试文件新增 `RuntimeContextFactory::create` 并调用 `RuntimeContext::new`；
2. 在非 factory 文件调用 `RuntimeContextFactory::prepare`；
3. 在非 factory 文件调用 `RunInstance::new`；
4. Main caller 绕过 `RunFactory::create`；
5. Derived caller 拆散 `RunInstance`；
6. `RunCreationRequest` 添加 live Port；
7. Runtime crate-root 导出 `RunFactory`；
8. 新增改名后的 fat Loop capability 聚合类型。

总编排验证通过 `.agents/hooks/check-architecture-guards.sh --fast` 运行，故意违规时同样必须返回 `exit 2`。

## 7. L1–L4 测试设计

### 7.1 L1：纯值与局部策略

保留或补齐：

- `RunSpec::validate_against`：Derived capability 不得超过 parent；
- `RunCreationRequest::new`：有 parent 时执行 ceiling 校验，无 parent 时保持纯 Main request；
- binding mode 的值对象行为；
- `SessionState::snapshot_for_run` revision 与字段冻结。

L1 不直接测试私有 selector。selector 的真实结果由 L2 `RunFactory` 协作测试验证。

### 7.2 L2：Runtime application 模块协作

以 production `RunFactory::create` 为唯一入口，建立两组 fixture：

#### Session Run

验证：

- Session wiring 的 committed Context/Memory 被绑定；
- provider、interaction、reasoning、event sink 字段完整且未覆写；
- Tool Catalog/Execution、Policy、Task、Reflection History、Hook 来自同一 factory services；
- 每次 Run 创建独立 input buffer、cancel scope 与 usage tracker；
- SessionSnapshot 在 gate 下绑定到 committed session/config revision；
- 返回完整 `RunInstance`，Run parent 为空且 Execution 初始为空。

#### Derived Run

验证：

- parent facts 与 parent bindings 同时存在；缺任一侧返回 typed `ContextAssembly`；
- provider 由同一 factory 的 provider factory 按 role/model 构造；
- Context 使用 isolated workspace；
- Tool Catalog 被收缩为 restricted snapshot；
- Interaction 使用新的 parent-mediated adapter，而非复用 parent Arc；
- Hook 使用受限 adapter；
- reasoning 复制值而非共享 parent mutable Arc；
- event route 隔离，不覆写 parent sink；
- cancel 使用 child scope；skill-load state/session identity 按设计继承；
- parent_run_id 与 workspace 放入同一个 `RunInstance`。

错误路径覆盖 provider factory 缺失、skill catalog 缺失、unknown/disabled/no-model role、unknown model、Tool Catalog snapshot 失败和 capability escalation。

### 7.3 L3：Composition 与相邻契约

Composition 契约测试验证：

1. concrete Tool Catalog/Execution、Policy、Hook、Reflection History、Task 只在 Composition 创建；
2. `RuntimeContextFactory` 在 Composition 构造一次，并将同一 Arc 注入 Main Session runtime 与 Agent runner/Derived setup；
3. provider factory 与 skill catalog 通过 `with_derived_bindings` 进入同一 factory，不创建 Derived 专用 factory 算法；
4. Context、Model、Tool、Event、Control 的字段从 Composition 到 Runtime bootstrap 无丢失、无覆写、无第二注入路径；
5. Runtime crate-root 精确 façade 足够完成 Composition 装配，但不暴露内部 `RunFactory`、bindings 或 `RunInstance`。

同一字段完整性断言定义一次，通过 Main/Derived fixture 复用，避免复制契约逻辑。

### 7.4 L4：Main 与 Derived 场景

#### Main 场景

```text
Composition assembly
  → SessionState snapshot
  → SessionRunBindings
  → RunFactory::create
  → RunInstance
  → RunLauncher::launch
  → 单一 Loop
```

场景断言：

- 输入经唯一 Run input buffer 激活；
- 模型、Tool、Event、Control 使用装配好的 Context；
- Loop 启动前没有拆包或第二 Context 构造；
- 终态来自同一个 Run/Execution 聚合。

#### Derived 场景

```text
Parent RunInstance
  → ParentRunFacts + ParentRunBindings
  → same RuntimeContextFactory
  → RunFactory::create
  → restricted Derived RunInstance
  → same RunLauncher
  → same Loop
```

场景断言：

- parent relation 正确；
- capability 无扩权；
- Derived Context/Workspace/Event/Interaction 与 sibling、parent 隔离；
- Tool/Hook/Reasoning 按受限语义绑定；
- 没有 Derived 专用 Loop、launcher 或 Context assembler。

L4 只证明最终组合，不替代 L1 ceiling 与 L2/L3 字段完整性契约。

## 8. 文件归属

预计修改：

- `agent/features/runtime/src/application/run/context_factory.rs`：删除 test-only 并行装配算法，保留唯一生产准备算法；
- `agent/features/runtime/src/application/run/context_factory_tests.rs`：迁移到 `RunFactory` 协作测试；
- `agent/features/runtime/src/application/run/context_tests.rs`：通过 production factory fixture 创建 Context；
- `agent/features/runtime/src/application/run/derived/tests/*.rs`：迁移 Derived fixture；
- `agent/features/runtime/src/application/client/from_args.rs` 及相关测试：删除测试 direct-create 使用；
- `agent/features/runtime/src/application/loop_engine/chat/*_tests.rs`：只迁移依赖 direct-create/with_hooks 的相关 fixture；
- `agent/features/runtime/src/application/run/tests/run_factory_support.rs` 与聚焦的 test-only fixture 子文件：承载 owning-layer typed fixture；
- `agent/composition/tests/main_session_wiring.rs` 或聚焦的 Runtime wiring contract：补同一 factory Arc 与字段完整性契约；
- `.agents/hooks/check-runtime-capability-assembly.sh`：收紧正向结构与测试旁路规则；
- `.agents/hooks/check-runtime-capability-assembly-tests.sh`：增加故意违规证明；
- `docs/design/03-engineering/01-architecture-guards.md`：同步 Guard 行为与证据；
- `docs/design/03-engineering/04-testing-and-coverage.md`：登记本能力 L0–L4 证据矩阵；
- `docs/design/02-modules/runtime/07-runtime-ownership-and-assembly.md`：修正 `RuntimeContextFactory::create` 过期伪代码和唯一调用链描述。

fixture 使用显式 `tests/run_factory_support.rs + tests/run_factory_support/` 物理边界，避免新增会被生产 Guard 扫描的 `testing.rs` 路径。

## 9. 实施顺序

1. 建立 Guard 和源码契约的失败证据，证明测试旁路当前可存在；
2. 建立/整理 owning-layer production-chain fixture；
3. 迁移 Session Run 的 direct-create 测试；
4. 迁移 Derived Run 的 direct-create 测试；
5. 迁移 client/loop 相关 fixture；
6. 删除 test-only `create` 与 selector；
7. 收紧 `prepare`、`RunInstance::new`、RunFactory/launcher 唯一调用点 Guard；
8. 补 Composition L3 契约；
9. 补 Main/Derived L4 场景；
10. 同步 Guard registry、Runtime 设计与测试证据矩阵；
11. 执行完整验证并检查退役路径与死代码。

## 10. 验证门禁

按顺序保留首次结果：

1. `scripts/setup-dev-env.sh --check`；
2. `bash .agents/hooks/check-runtime-capability-assembly-tests.sh`；
3. `bash .agents/hooks/check-runtime-capability-assembly.sh`；
4. Runtime L1 定向测试；
5. Runtime RunFactory/Context L2 定向测试；
6. Composition RuntimeContext wiring L3 定向测试；
7. Main/Derived L4 场景测试；
8. `cargo fmt --all -- --check`；
9. `cargo test -p runtime --lib`；
10. `cargo test -p composition --tests`；
11. `cargo run -p xtask -- production-reachability .`；
12. `cargo clippy --workspace --all-targets -- -D warnings`；
13. `.agents/hooks/check-architecture-guards.sh --full`；
14. `cargo test --workspace`。

首次失败不得被重跑成功覆盖。若完整 workspace 门禁暴露与本改动无关的既有失败，必须记录原始证据并区分 blocker，不得宣称全部通过。

## 11. 完成定义

- `RuntimeContextFactory` 不再存在 test-only Context 创建算法或 selector；
- 所有相关测试最终通过 `RunFactory::create` 取得 `RunInstance`；
- `RuntimeContextFactory::prepare` 只有 `RunFactory` 一个调用 owner；
- `RuntimeContext::new` 与 `RunInstance::new` 各自只有一个允许位置；
- Main 与 Derived 经同一 factory、RunFactory、launcher 和 Loop Engine；
- Derived capability 不扩权，且 Context/Model/Tool/Event/Control 字段完整、无覆写、无旁路；
- 每条新增 Guard 都有单 Guard 与总编排 `exit 2` 的故意违规证明；
- L0–L4 证据矩阵无未解释空白；
- Guard registry、Runtime 设计与测试设计同步；
- 生产可达性、clippy、完整守卫与 workspace 测试通过，或有明确、可验证的外部 blocker 记录。
