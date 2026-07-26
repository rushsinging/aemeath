# #1248 Runtime Interaction、Hook 与 Reasoning 装配实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 通过统一 `RuntimeContextFactory` 按 `RunSpec` 装配 Interaction、Hook 与 Reasoning 能力，使所有 Run 使用同一 Loop；补齐四类 interaction completion、Stop Hook 三分支状态语义和派生 Run 独立 reasoning。

**Architecture:** 不新增 Main/Sub 生产类型或 Loop 分支。Composition 构造长生命周期 `RuntimeServices` 与唯一 `RuntimeContextFactory`；Runtime application 在 Run 创建点提供 `RunSpec`、窄 session snapshot 和可选父能力。Factory 按能力模式绑定 client/parent-mediated/unavailable Interaction、完整/边界 Hook 和 adaptive/fixed/inherited/no-op ReasoningPort。`Run` 独占 interaction、Stop Hook 与终态状态迁移；执行工作数据仍暂留现有 adapter，待 #1397 收入 `RunExecutionState`。

**Tech Stack:** Rust 2021、Tokio、async-trait、runtime/workflow/hook/composition crates、Cargo workspace architecture guards

**Issue:** [#1248](https://github.com/rushsinging/aemeath/issues/1248)  
**Parent:** [#878](https://github.com/rushsinging/aemeath/issues/878)  
**Blocks:** [#1397](https://github.com/rushsinging/aemeath/issues/1397)

---

## 1. 开发门禁与冻结边界

实施期间逐项维护；PR 前必须完成或在 PR 中记录可验证的不适用理由。

- [ ] Main/Sub 使用同一 `run_launcher`、`run_loop` 与 coordinator；生产控制流不按 `RunKind` 分叉。
- [ ] `RuntimeContextFactory` 是 `RuntimeContext` 唯一生产构造入口；删除 `RuntimeContextParts` 与 `assemble_main_runtime_context`。
- [ ] Interaction 的 UserQuestions、ToolApproval、PlanApproval、HardPause reply/cancel 均由统一 coordinator 驱动 `Run` continuation。
- [ ] parent-mediated interaction 可生产装配；缺失能力返回 typed unavailable/failed，禁止永久等待或 `NotFound` 伪装。
- [ ] Stop Hook Continue、Block、ExecutionFailed 最终 outcome 由共享 Loop 驱动 `Run` 状态迁移；第 16 次阻断进入 typed Failed，不伪造 Completed。
- [ ] 派生 Run 使用独立 Fixed/Inherit/NoOp reasoning instance；`observe` 不推进 graph，且不修改父 requested。
- [ ] 不接 Provider requested→effective clamp；继续由 #1142 承接。
- [ ] 不在本 Issue 删除 `MainRunPort`、`SubAgentRun`、fat `RunLoopPort`；由 #1397/#1399 收口。
- [ ] L0-L4 相邻测试完整，check/clippy/fmt/architecture guards 全通过。
- [ ] 回写 Target 文档与 Migration Governance 的 O10/R8/T5；不自行关闭 Issue。

### 当前差异事实

1. `RuntimeContextParts`、公开 `RuntimeContext::new`、`MainSessionShell::assemble_main_runtime_context` 与 `derive_sub_run` 手填 context 并存。
2. `RunSpec` 有 `Interactive/NonInteractive`，但当前派生路径固定装配 `InteractionBridge::disabled()`；没有 parent-mediated adapter，也把 unavailable 表达成 `NotFound`。
3. `InteractionBridge` 只验证四种 reply 类型；生产请求仅 UserQuestions 可达，另外三种 continuation 只存在于领域测试。
4. Stop Hook 在 Main adapter 的 `invoke_model`/`finalize` 中转为 `ModelStep::StopHookBlocked`，计数藏在 `StuckGuard`；派生 Run 的 finalization 在共享 Loop 之外。
5. Workflow 只有 `AdaptiveReasoningPort`；派生 context 直接共享父 Arc，会推进父 graph 并修改父 requested。

---

## Task 1：冻结统一装配输入与能力模式

**Files:**
- Modify: `agent/features/runtime/src/domain/agent_run/spec.rs`
- Modify: `agent/features/runtime/src/domain/agent_run/tests.rs`
- Create: `agent/features/runtime/src/application/runtime_context_factory.rs`
- Create: `agent/features/runtime/src/application/runtime_context_factory_tests.rs`
- Modify: `agent/features/runtime/src/application.rs`

- [ ] **Step 1：先写 RunSpec 能力测试（RED）**

为 interaction、hook、reasoning 增加明确模式和值域测试：独立 client interaction、parent-mediated interaction、unavailable；Full/BoundaryOnly Hook；Adaptive/Fixed/Inherit/NoOp Reasoning。派生 spec 的每个模式必须通过 capability ceiling 校验，放宽父能力返回 `CapabilityEscalation`。

- [ ] **Step 2：运行 RED**

Run: `cargo test -p runtime --lib domain::agent_run -- --nocapture`  
Expected: FAIL，新增能力模式尚不存在。

- [ ] **Step 3：实现领域模式**

删除以 Main/Sub 固定能力的 builder 约束；`RunKind` 暂只保留兼容/观测，不参与 factory 分支。模式命名必须表达能力语义，不能使用 `MainInteraction`/`SubReasoning`。

- [ ] **Step 4：先写 factory 选择契约（RED）**

以 recording factories 验证：相同入口根据 `RunSpec` 选择 adapter；派生输入必须携带父 capability snapshot；缺少 parent-mediated source 时返回 typed `RuntimeContextAssemblyError::InteractionUnavailable`；禁止 fallback 到 disabled/no-op。

- [ ] **Step 5：建立 `RuntimeContextFactory` 骨架**

Factory 接收 `RuntimeServices`、Run session snapshot、`RunSpec` 和可选 parent capabilities；只负责选择和构造活契约，不启动 Loop、不修改 session state。此 Task 先返回可验证的 binding decision，后续 Task 接入完整 context。

- [ ] **Step 6：运行 GREEN**

Run: `cargo test -p runtime --lib runtime_context_factory -- --nocapture`  
Expected: PASS。

---

## Task 2：实现 Workflow 固定、继承与 NoOp ReasoningPort

**Files:**
- Modify: `agent/features/workflow/src/domain/reasoning_port.rs`
- Create: `agent/features/workflow/src/domain/reasoning_port_tests.rs`
- Modify: `agent/features/workflow/src/lib.rs`

- [ ] **Step 1：外置既有测试并写失败矩阵（RED）**

覆盖：Fixed 保持构造时的 requested；Inherit 仅在构造时读取一次父 requested；NoOp 固定发布构造值（默认 `Off`）；三者 `observe` 不推进 graph；子 `set_level/reset_default_level` 不修改父 port。

- [ ] **Step 2：运行 RED**

Run: `cargo test -p workflow reasoning_port -- --nocapture`  
Expected: FAIL，只有 Adaptive 实现。

- [ ] **Step 3：实现独立无 graph port**

复用一个不含 `ReasoningGraph` 的最小状态实现；Fixed/Inherit 若运行语义完全一致，仅以清晰 factory/构造函数表达来源，禁止复制 trait 实现。NoOp 的写操作保持 no-op，并返回其固定 requested。

- [ ] **Step 4：运行 GREEN**

Run: `cargo test -p workflow reasoning_port -- --nocapture`  
Expected: PASS，测试能证明父状态和 graph 不受影响。

---

## Task 3：将 RuntimeContext 构造收口到唯一 Factory

**Files:**
- Modify: `agent/features/runtime/src/application/runtime_context.rs`
- Modify: `agent/features/runtime/src/application/runtime_context_tests.rs`
- Modify: `agent/features/runtime/src/application/runtime_context_factory.rs`
- Modify: `agent/features/runtime/src/application/runtime_context_factory_tests.rs`
- Modify: `agent/features/runtime/src/application/client/accessors.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/setup.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/tests/runtime_context_derivation.rs`
- Modify: `agent/composition/src/runtime.rs`
- Modify: `agent/composition/src/runtime_tests.rs`

- [ ] **Step 1：写唯一构造与能力 identity 测试（RED）**

验证独立 Run 与派生 Run 均经同一 factory；context 构造后不可替换；派生 cancel、tool、policy、memory、workspace、interaction、hook、reasoning 不扩权；reasoning instance 不与父 Arc 相同。

- [ ] **Step 2：删除无语义参数包**

删除 `RuntimeContextParts`；将 `RuntimeContext::new` 收窄为 factory 所在模块可见的明确构造函数。测试 fixture 必须经 test-only factory，不能恢复公共万能构造器。

- [ ] **Step 3：建立 `RuntimeServices` 与窄 session snapshot**

把稳定 factory/port 与会话动态值分开。只迁移本 Issue 装配 Interaction/Hook/Reasoning 所需字段；不得在此顺带完成 #1397 的 `SessionState`/`RunExecutionState` 全量重构。

- [ ] **Step 4：切换两个生产创建点**

独立 Run 和派生 Run 调同一 `RuntimeContextFactory::assemble`。删除 `assemble_main_runtime_context` 和 `derive_sub_run` 内手填 context；`derive_sub_run` 可暂保留为准备 launch input 的薄协调函数。

- [ ] **Step 5：Composition 契约测试**

证明 Composition 只构造一次 `RuntimeServices`/factory，并将同一个 factory 注入 Runtime；Runtime 决定快照时机。禁止把 `from_args.rs` 整体搬进 Composition。

- [ ] **Step 6：运行 GREEN 与静态搜索**

Run: `cargo test -p runtime runtime_context -- --nocapture`  
Run: `cargo test -p composition runtime -- --nocapture`  
Run: `rg 'RuntimeContextParts|assemble_main_runtime_context|RuntimeContext::new' agent --glob '*.rs'`  
Expected: 测试 PASS；生产引用为零，允许的 test-only 内部 fixture 必须有明确命名。

---

## Task 4：建立统一 InteractionPort coordinator 与 parent-mediated adapter

**Files:**
- Modify: `agent/features/runtime/src/application/interaction.rs`
- Modify: `agent/features/runtime/src/application/interaction_tests.rs`
- Create: `agent/features/runtime/src/application/interaction_coordinator.rs`
- Create: `agent/features/runtime/src/application/interaction_coordinator_tests.rs`
- Create: `agent/features/runtime/src/adapters/parent_interaction.rs`
- Create: `agent/features/runtime/src/adapters/parent_interaction_tests.rs`
- Modify: `agent/features/runtime/src/adapters.rs`
- Modify: `agent/features/runtime/src/application/runtime_context_factory.rs`

- [ ] **Step 1：定义 typed unavailable 与端口契约测试（RED）**

将具体 `InteractionBridge` 从 RuntimeContext 能力面收敛为 Runtime-owned `InteractionPort`。Client adapter 与 parent-mediated adapter 复用同一契约套件：register、reply、cancel、drain、重复 completion、错误 variant；Unavailable adapter 必须立即返回 typed Failed/Unavailable。

- [ ] **Step 2：实现 parent-mediated adapter**

请求保留原 `RunId`、`InteractionRequestId`、body 和 continuation identity，通过父级 request/reply seam 转发；父终止、channel drop 或能力缺失立即完成为 typed failure/cancel，不能悬挂 waiter。

- [ ] **Step 3：实现 interaction coordinator**

Coordinator 顺序固定：创建 request → `Run::begin_interaction` → port register/publish → 等待 completion/control → 校验 reply → `Run::complete_interaction` 或 cancel/fail → 发布领域事件。四类 body 共用一个穷尽 match，不复制四套状态机。

- [ ] **Step 4：运行 GREEN**

Run: `cargo test -p runtime interaction -- --nocapture`  
Expected: PASS。

---

## Task 5：接通四类 Interaction 生产触发与 continuation

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/tests.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/tools.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/non_agent.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/tests.rs`
- Modify: `agent/features/runtime/src/application/run_launcher_tests.rs`

- [ ] **Step 1：写四类 reply/cancel L2/L4 表驱动测试（RED）**

每类均覆盖 reply 与 cancel：UserQuestions 完成 tool call；ToolApproval 恢复审批点；PlanApproval 恢复 context preparation；HardPause 恢复执行或按 cancel 终止。断言 request identity、pending continuation、状态、事件和 terminal outcome。

- [ ] **Step 2：把 ToolApproval 触发接入 coordinator**

Policy `RequireApproval` 不再在 adapter 中压成文本/拒绝；创建 `ToolApproval` request 并保存 `ContinueToolApproval(call_id)`。当前 v0.1.0 AllowAll 不妨碍 fake policy 场景证明生产路径可达。

- [ ] **Step 3：把 PlanApproval 与 HardPause 触发接入 coordinator**

Plan mode 与 StuckGuard hard pause 只产生 typed interaction intent，状态迁移由 coordinator/Run 完成。若当前产品策略明确 HardPause cancel 为 RunFailed，应使用 typed domain reason，而非 adapter 字符串。

- [ ] **Step 4：让固定输入派生 Run 复用同一 coordinator**

派生 Run 的 interaction 行为完全取决于 factory 装配结果；Loop 不检查 parent id 或 RunKind。parent-mediated 可回复；unavailable 立即 Failed。

- [ ] **Step 5：运行 GREEN**

Run: `cargo test -p runtime interaction -- --nocapture`  
Run: `cargo test -p runtime run_launcher -- --nocapture`  
Expected: PASS，四种 body 的 reply/cancel 均有生产链路场景。

---

## Task 6：把 Stop Hook outcome 迁入共享 Loop 与 Run 状态机

**Files:**
- Modify: `agent/features/runtime/src/domain/agent_run/state.rs`
- Modify: `agent/features/runtime/src/domain/agent_run/domain.rs`
- Modify: `agent/features/runtime/src/domain/agent_run/tests.rs`
- Create: `agent/features/runtime/src/application/stop_hook_coordination.rs`
- Create: `agent/features/runtime/src/application/stop_hook_coordination_tests.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/tests.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/finalize.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`

- [ ] **Step 1：先写 Run 状态机测试（RED）**

为 finishing/stop decision 建立 typed transition：Proceed → Completed；Block → 记录 count、提交当前 Step、注入 feedback 并回 PreparingContext；ExecutionFailed 使用 Hook 最终 typed outcome 进入同一 block 分支；第 16 次 → Failed(`StopHookRetryExhausted`)。计数属于 `Run`，不属于 `StuckGuard`。

- [ ] **Step 2：实现 Stop Hook coordinator**

Coordinator 只消费 `HookPort`/Hook PL，返回 Runtime-owned typed decision；保留 block detail/messages，禁止用 reason 字符串区分主动 Block 与 ExecutionFailed。Hook 内部三次 retry 仍归 Hook BC。

- [ ] **Step 3：共享 Loop 触发 Stop Hook**

模型尝试完成时由 Loop 调 coordinator，再请求 `Run` 迁移。删除 `ModelStep::StopHookBlocked` 这种 adapter 预解释结果和 `StuckGuard::record_stop_hook_block`；Main/Sub adapter 只提供 Hook 调用所需窄数据及副作用端口。

- [ ] **Step 4：统一派生 Run finalization**

移除共享 Loop 结束后再次单独执行 Sub stop hook 的路径；SubRunStart/Stop 边界 hook 可保留为 lifecycle hook，但不得重复解释 Stop directive。

- [ ] **Step 5：运行 L2/L4 GREEN**

Run: `cargo test -p runtime stop_hook -- --nocapture`  
Run: `cargo test -p hook --lib -- --nocapture`  
Expected: Continue、主动 Block、ExecutionFailed、第 15/16 次边界、控制抢占和派生 Run Failed 回传全部 PASS。

---

## Task 7：统一 reasoning 的 Loop 消费并证明派生独立性

**Files:**
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/tests.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/tests/runtime_context_wiring.rs`

- [ ] **Step 1：写共享 Loop observation 测试（RED）**

相同 Loop 时机统一发送 UserMessage、TextOnly、TurnBoundary；ToolCompleted 不作为 reasoning 信号且不接线。Adaptive port 可改变 requested；Fixed/Inherit/NoOp 返回稳定 observation。测试不得读取 Workflow graph 私有类型以外的内部状态。

- [ ] **Step 2：移动 reasoning 观察点到共享 Loop/coordinator**

从 `MainRunPort` 特有实现移除 observation；RunLoopPort 只提供已经装配的 `ReasoningPort` 或窄方法，最终由 #1397 将该 accessor 收入统一 adapter。

- [ ] **Step 3：验证父子隔离**

派生 Run 多轮 observe/set/reset 后，父 `current_requested_level` 不变；派生 Run context 中不存在 graph；两个 sibling 不共享 mutable reasoning。

- [ ] **Step 4：运行 GREEN**

Run: `cargo test -p runtime reasoning -- --nocapture`  
Run: `cargo test -p workflow reasoning_port -- --nocapture`  
Expected: PASS。

---

## Task 8：生产可达守卫与文档回写

**Files:**
- Modify: `.agents/architecture-guard-registry.json`
- Create or Modify: `.agents/hooks/check-runtime-capability-assembly.sh`
- Modify: `docs/design/02-modules/runtime/01-domain-model.md`
- Modify: `docs/design/02-modules/runtime/07-runtime-ownership-and-assembly.md`
- Modify: `docs/design/02-modules/hook/01-run-loop-integration.md`
- Modify: `docs/design/02-modules/workflow/01-reasoning-graph.md`
- Modify: `docs/design/01-system/03-context-map.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`

- [ ] **Step 1：先写守卫反例 fixture**

守卫至少拒绝：生产 `RuntimeContextParts`/公开 `RuntimeContext::new`；Main/Sub 命名 factory/assembler；派生 context 共享父 ReasoningPort；Loop 按 `RunKind`/parent id 选择 interaction/hook/reasoning；Stop block 计数回到 StuckGuard；四类 interaction 仅存在 PL 而无 production trigger。

- [ ] **Step 2：注册并运行守卫 RED/GREEN**

Run: `bash .agents/hooks/check-runtime-capability-assembly.sh`  
Expected: 对反例 fixture FAIL，对仓库生产树 PASS。

- [ ] **Step 3：回写 Target 与 Migration**

更新：统一 Factory 的最终装配表；Interaction parent-mediated/unavailable；Hook 三分支和第 16 次 Failed；Fixed/Inherit/NoOp reasoning；O10/R8/T5 当前状态；#1142 延期边界；#1397/#1399 待退役类型。目标文档不得继续把 Main/Sub 当生产类型。

- [ ] **Step 4：核对 Issue 门禁**

将本计划第 1 节与 Issue 全部 checklist 逐项对照；若实现发现新 unavailable/cancel/hook failure/reasoning 继承差异，先追加到 #1248，再创建 PR。

---

## Task 9：最终验证与 PR 准备

- [ ] **Step 1：定向测试**

Run: `cargo test -p workflow`  
Run: `cargo test -p hook`  
Run: `cargo test -p runtime interaction`  
Run: `cargo test -p runtime stop_hook`  
Run: `cargo test -p runtime reasoning`  
Run: `cargo test -p composition runtime`

- [ ] **Step 2：Issue 指定编译门禁**

Run: `cargo check -p runtime -p workflow -p hook`  
Run: `cargo clippy -p runtime -p workflow -p hook --all-targets -- -D warnings`  
Run: `cargo fmt --all -- --check`

- [ ] **Step 3：架构与生产可达性**

Run: `bash .agents/hooks/check-architecture-guards.sh`  
Run: `rg 'RuntimeContextParts|assemble_main_runtime_context|InteractionBridge::disabled|record_stop_hook_block' agent --glob '*.rs'`  
Run: `rg 'RunKind::(Main|Sub)' agent/features/runtime/src/application --glob '*.rs'`

Expected: guards PASS；禁止符号生产引用为零；剩余 RunKind 引用若仅兼容/观测，必须在 PR Test plan 逐项解释并由 #1397 承接。

- [ ] **Step 4：全 workspace 回归**

Run: `cargo test --workspace`  
Expected: PASS。首次失败必须保留并分类，禁止以重跑成功覆盖。

- [ ] **Step 5：PR 前同步**

Run: `git pull origin main`  
处理冲突后重跑 Step 1-4。PR 使用模板，Refs #1248；Test plan 明列四类 interaction、Hook 三分支/15-16 边界、Reasoning 父子隔离、Factory/guard 与全部命令。不得自行合并或关闭 Issue。

---

## 2. 与后续 Issue 的边界

### 本 Issue 必须完成

- 按 `RunSpec` 的 Interaction/Hook/Reasoning 能力装配。
- 唯一 `RuntimeContextFactory` 及旧参数包/双 assembler 删除。
- 四类 interaction completion 的生产链路。
- Stop Hook typed outcome 共享 Loop 状态迁移。
- Fixed/Inherit/NoOp reasoning 与父子隔离。

### 留给 #1397/#1399

- 删除 `MainRunPort`、`SubAgentRun`、fat `RunLoopPort`。
- 提取完整 `RunExecutionState`，消除剩余 Main/Sub adapter。
- 全面退役 `MainSessionShell`、`RunKind` 和 `from_args.rs` 第二 Composition Root。

### 留给 #1142

- Provider requested→effective reasoning clamp。
- effective value 同时冻结到 ContextRequest/InvocationRequest。
- Reasoning Graph 在 v0.2.0 的保留、接线或退役决策。
