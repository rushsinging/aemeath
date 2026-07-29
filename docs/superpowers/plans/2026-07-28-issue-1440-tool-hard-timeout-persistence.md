# #1440 所有 Tool 硬超时与调用收据持久化实施计划

> **执行要求：** 使用 `superpowers:subagent-driven-development`（优先）或 `superpowers:executing-plans` 逐任务实施；每个任务严格遵循 RED → GREEN → 重构 → 定向验证，不得跳过首次失败证据。

**目标：** 所有普通 Tool、Agent Tool、MCP Tool 与审批续执行统一经过 Runtime-owned 执行监督器；effective deadline 到达后前台按墙钟时间收敛为 typed 终态，同时 durable 保存已接受调用的 identity、真实状态与未确认副作用，使 `Glob("**/archify.mjs")` 一类运行中调用在强制中止或重启后不再从 Session 消失。

**架构：** Tools 继续只发布 descriptor、Invocation、Outcome 与 cancellation capability；Runtime 新增唯一 `ToolExecutionSupervisor`，负责 deadline、用户取消、grace period、重入保护、生命周期日志和终态映射。Context Management 新增 Tool call receipt mutation OHS，以同一 canonical Session backing 和 AtomicBlob writer 原子推进 `Pending → Running → terminal`；finalized Step 仍提交消息历史，但不再是调用 identity 的首次落盘点。阻塞/外部 adapter 分别提供真实的隔离与取消确认：Glob 分段可取消，Bash 管理进程组并 reap，MCP/Agent 未获远端或子 Run 终止确认时返回 `CancellationUnconfirmed`。

**技术栈：** Rust 2021、Tokio paused time、`tokio_util::CancellationToken`、Context canonical Session v4、Storage AtomicBlob、Unix process groups、Cargo workspace tests、Python/shell architecture guards。

---

## 开发前差异清单

实现开始前必须把以下已确认矛盾写入文档并在后续任务逐项关闭：

1. `docs/design/02-modules/runtime/05-recovery-semantics.md` 当前声明 ToolCall 进行态只在内存；#1440 已批准将“已接受调用的 durable receipt”作为 Session 对话事实保存，但仍不恢复 Run future 或自动重放。
2. `docs/design/02-modules/context-management/01-session.md` 当前 §6.2 禁止 Pending/Running 进入 Session；需改为禁止恢复可执行 future，同时允许 durable call ledger 保存状态。
3. `docs/design/02-modules/tools/02-ports-and-lifecycle.md` 已声明 Runtime 拥有 timeout，但当前 Main AskUser、Agent、Sub approval 等路径仍直调 `ToolExecutionPort`。
4. `agent/features/runtime/src/application/subagent/agent.rs::execute_call` 将 timeout 映射为 `Failure(Internal)`，且 `agent_calls.rs`、`tools.rs`、`subagent/runner/loop_run.rs` 存在旁路。
5. `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs::persist_step` 固定传 `vec![]` receipts。
6. `GlobTool::call` 在 async worker 上同步枚举全部路径；Bash 只 kill 直接 child；MCP 调用未传播 cancellation；Agent Tool 的入参 timeout 与 descriptor timeout 分散解释。

## 文件结构

### 新建

- `agent/features/runtime/src/application/tool_execution_supervisor.rs`：唯一 deadline/cancel/terminal supervisor。
- `agent/features/runtime/src/application/tool_execution_supervisor_tests.rs`：paused-time、阻塞隔离、确认/未确认终态与重入保护测试。
- `agent/features/context/src/domain/tool_receipt.rs`：call receipt 状态机、mutation 和 typed error。
- `agent/features/context/src/domain/tool_receipt_tests.rs`：单调性、幂等与非法倒退测试。
- `agent/features/context/tests/tool_receipt_persistence_contract.rs`：ContextPort → canonical Session → AtomicBlob reopen 契约。
- `agent/features/runtime/tests/tool_execution_lifecycle_scenario.rs`：Main/Sub/approval/Agent/MCP stub 统一监督场景。
- `.agents/hooks/check-runtime-tool-execution-supervisor.sh`：禁止 Runtime 生产旁路直调 Tool execution。
- `.agents/hooks/check-runtime-tool-execution-supervisor-tests.sh`：守卫负向探针。

### 修改

- Tools PL/adapter：`agent/features/tools/src/domain/{published_language.rs,context.rs}`、`agent/features/tools/src/adapters/{glob_tool.rs,bash.rs,grep.rs,mcp_tool.rs,agent_tool.rs}` 及对应测试。
- Runtime：`agent/features/runtime/src/application.rs`、`context_coordination.rs`、`context_coordination_tests.rs`、`subagent/{agent.rs,agent_tests.rs}`、`main_loop/looping/{tools.rs,non_agent.rs,agent_calls.rs,main_run_port.rs,loop_runner_tests.rs}`、`subagent/runner/{loop_run.rs,tests.rs}`、`runtime_context.rs`、`runtime_context_factory.rs` 及对应测试。
- Context：`agent/features/context/src/{domain.rs,ports.rs}`、`domain/session/envelope.rs`、`ports/context_port.rs`、`application/service.rs`、`adapters/{canonical_session.rs,in_memory_session.rs}` 及现有 contract/codec/recovery 测试。
- CLI 场景：`apps/cli/src/tui/effect/session/processing/handle.rs` 与父模块测试 `apps/cli/src/tui/effect/session/processing.rs`。
- Guard 注册：`.agents/hooks/check-architecture-guards.sh`、`docs/design/03-engineering/01-architecture-guards.md`。
- 目标文档：`docs/design/02-modules/runtime/{05-recovery-semantics.md,06-ports-and-adapters.md}`、`docs/design/02-modules/tools/02-ports-and-lifecycle.md`、`docs/design/02-modules/context-management/01-session.md`、`docs/design/02-modules/storage/README.md`、`docs/design/03-engineering/03-migration-governance.md`。

Storage crate 不新增 Session 业务类型；除非 contract 暴露 AtomicBlob 机制缺陷，否则不修改 `agent/features/storage/**` 生产代码。

---

### Task 1：冻结目标文档与行为—风险矩阵

**文件：**
- 修改：`docs/design/02-modules/runtime/05-recovery-semantics.md`
- 修改：`docs/design/02-modules/runtime/06-ports-and-adapters.md`
- 修改：`docs/design/02-modules/tools/02-ports-and-lifecycle.md`
- 修改：`docs/design/02-modules/context-management/01-session.md`
- 修改：`docs/design/02-modules/storage/README.md`
- 修改：`docs/design/03-engineering/03-migration-governance.md`

- [ ] **步骤 1：记录 Current → Target 差异**

逐项登记 Main 普通/并发 Tool、Sub、Agent、MCP、AskUser、approval continuation、Bash、Hook 包围执行、Run cancel/terminate 和 TUI 强制 abort。每项写明当前调用点、目标 supervisor 入口、cancellation declaration、底层停止确认来源、receipt 终态和测试层级。

- [ ] **步骤 2：修正文档冲突**

明确 durable call receipt 是“已接受执行事实”，不是可恢复 Run 状态：重启后不恢复 future、不自动重放，但保留 call identity、输入安全摘要、Pending/Running/terminal 状态和未确认副作用。Storage 仍只提供 AtomicBlob。

- [ ] **步骤 3：运行文档检查**

```bash
git diff --check
rg -n "ToolCall.*仅内存|PendingArgs.*Running.*不落盘|不做 checkpoint" docs/design/02-modules/runtime docs/design/02-modules/context-management
```

预期：旧绝对表述已改成不恢复 future/状态机；没有与 #1440 设计冲突的段落。

- [ ] **步骤 4：提交文档基线**

```bash
git add docs/design/02-modules/runtime docs/design/02-modules/tools docs/design/02-modules/context-management docs/design/02-modules/storage/README.md docs/design/03-engineering/03-migration-governance.md
git commit -m "docs: #1440 define tool deadline and receipt lifecycle"
```

### Task 2：扩展 Tool 与 Context Published Language

**文件：**
- 修改：`agent/features/tools/src/domain/published_language.rs`
- 修改：`agent/features/tools/src/domain/context.rs`
- 新建：`agent/features/context/src/domain/tool_receipt.rs`
- 新建：`agent/features/context/src/domain/tool_receipt_tests.rs`
- 修改：`agent/features/context/src/domain.rs`

- [ ] **步骤 1：先写 PL 的 RED 测试**

在 Tools PL 测试中覆盖：`ToolOutcome::{TimedOut,CancellationUnconfirmed}` 独立于 `Failure`/`Cancelled`；终态携带安全 reason、possible side effects、unfinished IDs 和 cleanup confirmation；`ExecutionScope.deadline` 只接受调用方计算后的绝对 deadline。把 `published_language.rs` 的内联测试中本任务触及部分迁到同级 `published_language_tests.rs`，符合当前无 inline tests 规范。

在 Context 新测试中覆盖：

- `Pending → Running → TimedOut` 合法；
- `Running → CancellationUnconfirmed` 保留 side effects/unfinished ID；
- 相同 mutation 幂等；
- terminal → Running、terminal → 另一 terminal 均拒绝；
- `TimedOut` 是独立 `ToolOutcomeKind`。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p tools tool_outcome_exposes_timeout_and_unconfirmed_terminals -- --nocapture
cargo test -p context tool_receipt_state_is_monotonic_and_idempotent -- --nocapture
```

预期：因类型/变体不存在而编译失败或断言失败。

- [ ] **步骤 3：实现最小 Published Language**

Tools 新增 typed timeout/unconfirmed outcome；不要把 Runtime timeout policy 放入 `ExecutionAdapter`。Context 定义 `ToolCallReceipt`、`ToolCallState`、`ToolReceiptMutation`、`ToolReceiptMutationReceipt/Error`，identity 至少包含 session/run/step/runtime call/provider call/tool/index/agent 标识。`StepReceipt` 复用同一 terminal payload，删除重复字段定义或改为从 terminal receipt 投影。

- [ ] **步骤 4：运行 GREEN 与边界守卫**

```bash
cargo test -p tools domain -- --nocapture
cargo test -p context tool_receipt -- --nocapture
bash .agents/hooks/check-tool-catalog-execution-boundary.sh
```

- [ ] **步骤 5：提交 PL**

```bash
git add agent/features/tools/src/domain agent/features/context/src/domain.rs agent/features/context/src/domain/tool_receipt.rs agent/features/context/src/domain/tool_receipt_tests.rs
git commit -m "feat: #1440 publish tool timeout and receipt states"
```

### Task 3：为 Context 增加 durable receipt mutation OHS

**文件：**
- 修改：`agent/features/context/src/ports/context_port.rs`
- 修改：`agent/features/context/src/ports.rs`
- 修改：`agent/features/context/src/application/service.rs`
- 修改：`agent/features/context/src/adapters/canonical_session.rs`
- 修改：`agent/features/context/src/adapters/in_memory_session.rs`
- 修改：`agent/features/context/src/domain/session/envelope.rs`
- 修改：`agent/features/context/src/application/service_tests.rs`
- 修改：`agent/features/context/src/domain/session/management_tests.rs`
- 修改：`agent/features/context/tests/context_port_contract.rs`
- 修改：`agent/features/context/tests/canonical_session_repository.rs`

- [ ] **步骤 1：写 Context mutation RED 契约**

新增/扩展测试证明 `ContextPort::advance_tool_receipt`：在 mutation gate 内读取 current generation、compare-and-advance、收集 Task/Workspace snapshot、AtomicBlob durable save 后才 publish；同 mutation 幂等且不增加 revision；冲突/非法倒退/写失败不改变 live generation。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p context advance_tool_receipt -- --nocapture
cargo test -p context canonical_session_repository -- --nocapture
```

- [ ] **步骤 3：实现 OHS 与 canonical mutation**

在 `ContextPort`/`SessionRepository` 新增一个窄 mutation 方法；`CanonicalSession` 的 `CommittedRunStep` 增加 `tool_receipts` ledger，`append_finalized_outcome` 只合并同 identity 的 terminal projection，不删除已 durable 的 receipt。保持一个 backing 和一个 mutation gate。

- [ ] **步骤 4：运行 Context 全量**

```bash
cargo test -p context --all-targets
```

- [ ] **步骤 5：提交 Context mutation**

```bash
git add agent/features/context
git commit -m "feat(context): #1440 persist tool receipt transitions"
```

### Task 4：升级 Session envelope 并验证恢复兼容

**文件：**
- 修改：`agent/features/context/src/domain/session/envelope.rs`
- 修改：`agent/features/context/tests/session_envelope_codec.rs`
- 修改：`agent/features/context/tests/session_persistence_service.rs`
- 修改：`agent/features/context/tests/session_recovery_scenarios.rs`
- 新建：`agent/features/context/tests/tool_receipt_persistence_contract.rs`

- [ ] **步骤 1：写 v3→v4、round-trip 与 future fail-closed RED**

测试：v3 缺 receipt ledger 解码为空；v4 Pending/Running/TimedOut/Unconfirmed 全字段 round-trip；unknown future schema 保留原 bytes；primary 损坏时 previous 恢复不丢 receipts；重复 reopen/mutation 不复制 identity。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p context session_envelope_codec -- --nocapture
cargo test -p context tool_receipt_persistence_contract -- --nocapture
```

- [ ] **步骤 3：实现 schema v4 与兼容 reader**

提升 `CURRENT_SESSION_SCHEMA_VERSION`，增加显式 v3 compatibility DTO/upgrade；不要通过 role 或 ToolUse/ToolResult 顺序猜 receipt。编码器只写 v4，future schema 继续 fail-closed。

- [ ] **步骤 4：验证 Context + Storage 相邻边界**

```bash
cargo test -p context --all-targets
cargo test -p storage atomic_blob_contract -- --nocapture
cargo test -p storage crash_recovery -- --nocapture
```

若 Storage 测试原样通过，记录“Storage 机制无生产改动”；不要为对称性修改 Storage。

- [ ] **步骤 5：提交 schema**

```bash
git add agent/features/context
git commit -m "feat(context): #1440 recover durable tool receipts"
```

### Task 5：建立唯一 Runtime ToolExecutionSupervisor

**文件：**
- 新建：`agent/features/runtime/src/application/tool_execution_supervisor.rs`
- 新建：`agent/features/runtime/src/application/tool_execution_supervisor_tests.rs`
- 修改：`agent/features/runtime/src/application.rs`
- 修改：`agent/features/runtime/src/application/context_coordination.rs`
- 修改：`agent/features/runtime/src/application/context_coordination_tests.rs`
- 修改：`agent/features/runtime/src/application/runtime_context.rs`
- 修改：`agent/features/runtime/src/application/runtime_context_factory.rs`
- 修改：对应 `runtime_context*_tests.rs`

- [ ] **步骤 1：写 supervisor RED 矩阵**

使用 `#[tokio::test(start_paused = true)]` 和 scripted `ToolExecutionPort`，覆盖：

- descriptor、ExecutionScope、Run 与调用方 deadline 取最早；
- dispatch 前 durable Pending，执行前 durable Running；只有持久化成功才发后续生命周期；
- cooperative timeout 触发 child cancellation，grace 内确认映射 TimedOut；
- cooperative 未确认和 non-cooperative 均映射 CancellationUnconfirmed；
- 用户取消映射 Cancelled/Unconfirmed，不与 timeout 混淆；
- non-cooperative 同 identity/危险同名重入被拒绝，确认 terminal 后释放；
- receipt 持久化失败不调用 Tool；terminal 持久化失败返回明确 Runtime error；
- 一个永不完成的 Tool 不阻塞 paused timer、取消处理或另一个 Tool。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p runtime tool_execution_supervisor -- --nocapture
```

- [ ] **步骤 3：实现 supervisor**

Supervisor 输入包含 run/step/call identity、descriptor、scope deadline、Run deadline、用户 cancel、`ContextCoordinator` receipt writer 与 `ToolExecutionPort`。它创建 per-call child cancellation，统一生成 typed terminal outcome；只在 cleanup confirmation 明确到达时记录 confirmed。日志使用 `crate::LOG_TARGET`，input 仅记录 tool 名、长度/安全 preview，禁止完整 JSON。

- [ ] **步骤 4：运行 GREEN**

```bash
cargo test -p runtime tool_execution_supervisor -- --nocapture
cargo test -p runtime context_coordination -- --nocapture
```

- [ ] **步骤 5：提交 supervisor**

```bash
git add agent/features/runtime/src/application
git commit -m "feat(runtime): #1440 add unified tool execution supervisor"
```

### Task 6：把 Main 普通、并发、AskUser 与 Agent 路径切到 supervisor

**文件：**
- 修改：`agent/features/runtime/src/application/subagent/agent.rs`
- 修改：`agent/features/runtime/src/application/subagent/agent_tests.rs`
- 修改：`agent/features/runtime/src/application/main_loop/looping/tools.rs`
- 修改：`agent/features/runtime/src/application/main_loop/looping/non_agent.rs`
- 修改：`agent/features/runtime/src/application/main_loop/looping/agent_calls.rs`
- 修改：相关同级测试；把本任务触及的 inline tests 迁至外置 `*_tests.rs`

- [ ] **步骤 1：写 Main 路径 RED 契约**

覆盖单 Tool、并发 batch、AskUser suspension、Agent Tool：均记录 Pending→Running→terminal；timeout 不是 `Failure(Internal)`；并发结果按原调用顺序；一个阻塞调用不阻止兄弟完成；Running UI 事件只能出现在 durable Running 之后。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p runtime main_tool_execution_uses_supervisor -- --nocapture
cargo test -p runtime main_parallel_tool_timeout_preserves_sibling_result -- --nocapture
```

- [ ] **步骤 3：迁移调用点**

删除 `Agent::execute_call` 自有 `tokio::time::timeout`，改为持有/调用 supervisor。`execute_non_agent`、AskUser 和 `execute_one_agent` 不再直接调用 `ToolExecutionPort::execute`。Hook/Policy 仍在 supervisor 外按既有顺序执行，但一旦调用被接受，receipt 必须先落盘；被拒绝/Hook block 也生成 Denied terminal receipt。

- [ ] **步骤 4：运行 Runtime Main 回归**

```bash
cargo test -p runtime application::main_loop::looping -- --nocapture
cargo test -p runtime application::subagent::agent -- --nocapture
```

- [ ] **步骤 5：提交 Main 迁移**

```bash
git add agent/features/runtime/src/application/main_loop agent/features/runtime/src/application/subagent
git commit -m "refactor(runtime): #1440 supervise all main tool calls"
```

### Task 7：把 Sub 与 approval continuation 切到 supervisor

**文件：**
- 修改：`agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- 修改：`agent/features/runtime/src/application/subagent/runner/tests.rs`
- 修改：`agent/features/runtime/src/application/subagent/runner/tests/runtime_context_wiring.rs`
- 修改：`agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- 修改：`agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`

- [ ] **步骤 1：写旁路 RED 测试**

Sub approval continuation 和 Main approval continuation 分别断言 approved call 经同一 supervisor，继承原 absolute deadline，不重新分配；取消/超时 taxonomy 与普通调用一致；approval denial 写 Denied receipt 且不执行。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p runtime sub_approval_continuation_uses_tool_supervisor -- --nocapture
cargo test -p runtime main_approval_continuation_uses_tool_supervisor -- --nocapture
```

- [ ] **步骤 3：迁移直调点**

替换 `subagent/runner/loop_run.rs` 和 `main_run_port.rs` 中的 `ToolExecutionPort::execute` 直调；确保 Main/Sub 的 `ExecutionScope` 正确携带 invocation source、parent run 和调用 deadline。

- [ ] **步骤 4：运行 Main/Sub 回归**

```bash
cargo test -p runtime application::subagent::runner -- --nocapture
cargo test -p runtime application::main_loop::looping::loop_runner -- --nocapture
```

- [ ] **步骤 5：提交 continuation 迁移**

```bash
git add agent/features/runtime/src/application/subagent/runner agent/features/runtime/src/application/main_loop/looping
git commit -m "refactor(runtime): #1440 supervise approval continuations"
```

### Task 8：修复 Glob 与文件类 Tool 的 executor 饥饿

**文件：**
- 修改：`agent/features/tools/src/adapters/glob_tool.rs`
- 新建：`agent/features/tools/src/adapters/glob_tool_tests.rs`
- 修改：`agent/features/tools/src/adapters/grep.rs`
- 修改：`agent/features/tools/src/adapters/grep_tests.rs`
- 修改：`agent/features/tools/src/adapters/file_read.rs`
- 新建：`agent/features/tools/src/adapters/file_read_tests.rs`

- [ ] **步骤 1：写 Glob 精确复现 RED**

使用每测试唯一目录和可控 cancellation，构造大量/深层目录，搜索 `**/archify.mjs`；通过检查点/脚本化 walker 断言取消后停止继续枚举。另用 multi-thread Tokio fixture 证明搜索不占用 core worker，timer 和并行 Read 可推进；墙钟 timeout 只作死锁上限，不作为业务断言。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p tools glob_cancel_stops_archify_search_at_checkpoint -- --nocapture
cargo test -p tools glob_search_does_not_starve_async_executor -- --nocapture
```

- [ ] **步骤 3：实现可取消/隔离遍历**

用可分段检查 cancellation/deadline 的 walker 替换一次性 `glob::glob(...).collect()`；若必须使用 `spawn_blocking`，任务结果必须明确 cleanup confirmation，不能把 drop JoinHandle 当成停止。避免遍历 `.git`、target 等既有语义变化，除非输入 pattern 本身要求；不顺带改变 Glob 匹配契约。

- [ ] **步骤 4：审计 Read/Grep**

Read 保持 Tokio 文件 I/O，但大文本编号/base64 CPU 工作应移入隔离 worker或分段取消；Grep 子进程使用 child cancellation/kill/reap，不再依赖 drop `Command::output()`。为每项新增 cancellation/resource release 测试。

- [ ] **步骤 5：运行 Tools 文件测试**

```bash
cargo test -p tools glob -- --nocapture
cargo test -p tools grep -- --nocapture
cargo test -p tools file_read -- --nocapture
```

- [ ] **步骤 6：提交文件 Tool 修复**

```bash
git add agent/features/tools/src/adapters/glob_tool.rs agent/features/tools/src/adapters/glob_tool_tests.rs agent/features/tools/src/adapters/grep.rs agent/features/tools/src/adapters/grep_tests.rs agent/features/tools/src/adapters/file_read.rs agent/features/tools/src/adapters/file_read_tests.rs
git commit -m "fix(tools): #1440 isolate cancellable file operations"
```

### Task 9：让 Bash 终止进程组并确认 reap

**文件：**
- 修改：`agent/features/tools/src/adapters/bash.rs`
- 修改：`agent/features/tools/src/adapters/bash/tests.rs`
- 需要时修改：`agent/features/tools/Cargo.toml`、`Cargo.lock`（仅加入 Unix process-group 所需依赖）

- [ ] **步骤 1：写 Unix 进程树 RED 测试**

Bash fixture 启动父 shell + 后台孙进程，写出 PID；触发 cancel 与 deadline 后断言整个进程组终止、child 已 reap、stdout/stderr reader 收敛。测试通过 pid liveness/管道 EOF 条件等待，不用固定 sleep。非 Unix 锁定平台 fallback 的 explicit unconfirmed 语义。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p tools bash_cancel_terminates_process_group_and_reaps -- --nocapture
cargo test -p tools bash_timeout_terminates_process_group_and_reaps -- --nocapture
```

- [ ] **步骤 3：实现进程组清理**

Unix spawn 时建立独立 process group；取消/timeout 先发送 TERM，在 bounded grace 后 KILL，始终 wait/reap；返回 cleanup confirmation 给 supervisor。Bash 输入 `timeout` 只缩短 Runtime effective deadline，删除 adapter 与 Runtime 相互矛盾的双层终态解释；adapter 内可保留防御性 process deadline，但必须使用同一 absolute deadline。

- [ ] **步骤 4：运行 Bash 全量**

```bash
cargo test -p tools adapters::bash -- --nocapture
```

- [ ] **步骤 5：提交 Bash 修复**

```bash
git add agent/features/tools/src/adapters/bash.rs agent/features/tools/src/adapters/bash agent/features/tools/Cargo.toml Cargo.lock
git commit -m "fix(tools): #1440 terminate bash process groups on deadline"
```

### Task 10：接通 MCP 与 Agent 的取消确认

**文件：**
- 修改：`agent/features/tools/src/adapters/mcp_tool.rs`
- 修改/新建：`agent/features/tools/src/adapters/mcp_tool_tests.rs`
- 修改：`agent/features/tools/src/adapters/agent_tool.rs`
- 修改：`agent/features/tools/src/adapters/agent_tool_tests.rs`
- 修改：`agent/features/runtime/src/application/main_loop/looping/agent_calls.rs`
- 修改：`agent/features/runtime/src/application/subagent/runner/tests/runtime_context_wiring.rs`

- [ ] **步骤 1：写 MCP/Agent RED 契约**

MCP stub 覆盖协议明确确认取消与仅断开本地 future 两种结果；后者必须是 `CancellationUnconfirmed`。Agent runner 覆盖父 deadline 传播、child terminal Cancelled/Terminated 确认和 child 未在 grace 内终止三种结果；入参 timeout 只能缩短父 deadline，`0` 按现有 Agent 语义解释但不得突破父上限。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p tools mcp_tool_cancellation -- --nocapture
cargo test -p tools agent_tool_deadline -- --nocapture
cargo test -p runtime agent_tool_cancellation_confirmation -- --nocapture
```

- [ ] **步骤 3：实现 adapter 确认映射**

MCP 调用监听 `ctx.cancellation()` 并调用协议 cancellation（若 client 支持）；没有 ack 时明确 unconfirmed。Agent Tool 把 effective absolute deadline 传给 `AgentRunRequest`，父取消请求 child terminate，并以 typed `AgentRunTerminal` 判断确认状态；禁止仅因 parent future drop 就报告 Cancelled。

- [ ] **步骤 4：运行 Tools/Runtime 定向测试**

```bash
cargo test -p tools mcp -- --nocapture
cargo test -p tools agent_tool -- --nocapture
cargo test -p runtime agent_calls -- --nocapture
cargo test -p runtime runtime_context_wiring -- --nocapture
```

- [ ] **步骤 5：提交外部调用修复**

```bash
git add agent/features/tools/src/adapters/mcp_tool.rs agent/features/tools/src/adapters/mcp_tool_tests.rs agent/features/tools/src/adapters/agent_tool.rs agent/features/tools/src/adapters/agent_tool_tests.rs agent/features/runtime/src/application/main_loop/looping/agent_calls.rs agent/features/runtime/src/application/subagent/runner/tests/runtime_context_wiring.rs
git commit -m "fix: #1440 confirm mcp and agent cancellation outcomes"
```

### Task 11：把 terminal receipt 接回 Step finalization 与恢复投影

**文件：**
- 修改：`agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- 修改：`agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- 修改：`agent/features/runtime/src/application/context_coordination.rs`
- 修改：`agent/features/runtime/src/application/context_coordination_tests.rs`
- 修改：`agent/features/context/src/domain/session/restore.rs`
- 修改：`agent/features/context/src/domain/session/restore_tests.rs`
- 修改：`agent/features/context/tests/session_recovery_scenarios.rs`

- [ ] **步骤 1：写空 receipts 与恢复完整性 RED 测试**

Main/Sub finalized Step 必须从 durable ledger 投影 terminal receipts，不能传 `vec![]`。恢复场景构造 assistant ToolUse + Running/Unconfirmed receipt，断言 provider-safe 合成 result 明确写 timeout/unconfirmed，`is_error=true`，且绝不伪造成功；重复恢复结果幂等、顺序保持 provider call index。

- [ ] **步骤 2：运行 RED**

```bash
cargo test -p runtime persist_step_projects_terminal_tool_receipts -- --nocapture
cargo test -p context unfinished_tool_call_recovers_as_explicit_unconfirmed_result -- --nocapture
```

- [ ] **步骤 3：接通 finalization 与恢复投影**

`persist_step` 查询/持有本 Step 的 durable receipt snapshot 并传入 `append_finalized`；Context merge terminal projection。恢复只修复 provider 消息完整性，不恢复执行；Pending/Running 在异常重启后投影为 `CancellationUnconfirmed`，保留原 durable 状态和 unfinished identity 作为事实。

- [ ] **步骤 4：运行 Context/Runtime 恢复测试**

```bash
cargo test -p runtime context_coordination -- --nocapture
cargo test -p context session_recovery -- --nocapture
cargo test -p context session_envelope_codec -- --nocapture
```

- [ ] **步骤 5：提交 finalization**

```bash
git add agent/features/runtime/src/application agent/features/context/src/domain/session agent/features/context/tests/session_recovery_scenarios.rs
git commit -m "fix: #1440 finalize and recover tool receipts"
```

### Task 12：增加全链路场景与 auto-save 强制中止回归

**文件：**
- 新建：`agent/features/runtime/tests/tool_execution_lifecycle_scenario.rs`
- 修改：`agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`
- 修改：`agent/features/runtime/src/application/subagent/runner/tests.rs`
- 修改：`apps/cli/src/tui/effect/session/processing/handle.rs`
- 修改：`apps/cli/src/tui/effect/session/processing.rs`（父模块测试）

- [ ] **步骤 1：写 Glob + Read 并发 L4 RED 场景**

使用 scripted blocking Glob adapter 和快速 Read：持久化两个 Pending/Running；推进虚拟 deadline/用户取消；Read 成功保留，Glob TimedOut/Unconfirmed；Step 收敛；reopen 后两个 identity 和原顺序都存在。

- [ ] **步骤 2：写精确 `**/archify.mjs` 强制中止 RED 场景**

在 Tool Running 后模拟 processing shutdown/abort，再 reopen canonical session；断言 pattern 的安全摘要、call identity、Running→Unconfirmed 恢复投影存在，后续已接受 Read identity 也不丢。不要用 `**/bin/archify.mjs` 替代 fixture。

- [ ] **步骤 3：运行 RED**

```bash
cargo test -p runtime --test tool_execution_lifecycle_scenario -- --nocapture
cargo test -p cli archify_glob_running_survives_forced_abort -- --nocapture
```

- [ ] **步骤 4：调整 shutdown 行为**

`shutdown_and_save` 的 5 秒上限只负责等待 Runtime 已开始的 cancellation-shielded durable receipt handoff；超限 abort 前不得跳过已 accepted call 的 pending receipt。必要时由 Runtime 暴露窄 drain handle，不让 TUI 直接写 Session。

- [ ] **步骤 5：运行 L4 场景**

```bash
cargo test -p runtime --test tool_execution_lifecycle_scenario -- --nocapture
cargo test -p runtime main_parallel_tool_timeout_preserves_sibling_result -- --nocapture
cargo test -p runtime sub_approval_continuation_uses_tool_supervisor -- --nocapture
cargo test -p cli archify_glob_running_survives_forced_abort -- --nocapture
```

- [ ] **步骤 6：提交场景修复**

```bash
git add agent/features/runtime apps/cli/src/tui/effect/session/processing
git commit -m "test: #1440 cover tool timeout and forced-abort recovery"
```

### Task 13：增加生命周期日志与 supervisor 旁路守卫

**文件：**
- 修改：`agent/features/runtime/src/application/tool_execution_supervisor.rs`
- 修改：`agent/features/runtime/src/application/tool_execution_supervisor_tests.rs`
- 新建：`.agents/hooks/check-runtime-tool-execution-supervisor.sh`
- 新建：`.agents/hooks/check-runtime-tool-execution-supervisor-tests.sh`
- 修改：`.agents/hooks/check-architecture-guards.sh`
- 修改：`docs/design/03-engineering/01-architecture-guards.md`

- [ ] **步骤 1：写日志 RED 断言**

捕获 dispatch accepted、execution started、deadline reached、cancellation requested、confirmed/unconfirmed、terminal receipt committed；每条可关联 run/step/call/tool/effective timeout/elapsed/declaration/outcome。断言完整 input、API key 和超长路径不进入日志。

- [ ] **步骤 2：写守卫负向探针**

守卫扫描 Runtime 生产源码中的 `.execute(invocation, ...)`/`ToolExecutionPort::execute`，仅允许 supervisor 文件；测试在 Main、Sub、approval 各注入一条直调并断言 exit 2。排除测试源码，不使用不断增长的路径白名单。

- [ ] **步骤 3：运行 RED**

```bash
cargo test -p runtime tool_lifecycle_log -- --nocapture
bash .agents/hooks/check-runtime-tool-execution-supervisor-tests.sh
```

- [ ] **步骤 4：实现安全日志并注册守卫**

正常取消/完成用 debug，unconfirmed 用 warn，durable failure 按影响用 warn/error；所有调用显式 `target: crate::LOG_TARGET`。在 guard registry 文档登记 owner、分类、负向测试与退出条件。

- [ ] **步骤 5：运行日志/守卫验证**

```bash
cargo test -p runtime tool_execution_supervisor -- --nocapture
bash .agents/hooks/check-runtime-tool-execution-supervisor.sh
bash .agents/hooks/check-runtime-tool-execution-supervisor-tests.sh
bash .agents/hooks/check-log-target-prefix.sh
```

- [ ] **步骤 6：提交日志与守卫**

```bash
git add agent/features/runtime/src/application/tool_execution_supervisor.rs agent/features/runtime/src/application/tool_execution_supervisor_tests.rs .agents/hooks docs/design/03-engineering/01-architecture-guards.md
git commit -m "guard: #1440 enforce supervised tool execution"
```

### Task 14：退役旧 timeout/空 receipt 旁路

**文件：**
- 修改：`agent/features/runtime/src/application/subagent/agent.rs`
- 修改：`agent/features/runtime/src/application/main_loop/looping/{tools.rs,non_agent.rs,agent_calls.rs,main_run_port.rs}`
- 修改：`agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- 修改：`agent/features/tools/src/adapters/{bash.rs,web_fetch.rs}`（只删除与统一 supervisor 冲突的 Runtime 终态解释；保留 transport 自身上限）
- 修改：`.agents/hooks/check-runtime-tool-execution-supervisor.sh`

- [ ] **步骤 1：检索旧路径并形成失败清单**

```bash
rg -n "tokio::time::timeout|timeout_at|ToolExecutionPort::execute|\.execute\(invocation|receipts:\s*vec!\[\]|vec!\[\]" agent/features/runtime/src agent/features/tools/src/adapters
```

逐项分类为 Tool execution policy、transport/process safety timeout、测试上限或无关 timeout；只退役第一类，禁止机械删除网络连接上限和测试死锁上限。

- [ ] **步骤 2：删除重复 policy**

移除 `call_tool_with_timeout` 测试辅助旧语义、`Agent::execute_call` timeout、Agent/AskUser/approval 直调和 `persist_step` 空 receipts。Bash/WebFetch 内只保留 adapter 自身资源清理/transport safety，不再生成与 supervisor 冲突的 terminal taxonomy。

- [ ] **步骤 3：证明零旁路**

```bash
bash .agents/hooks/check-runtime-tool-execution-supervisor.sh
rg -n "tool.call execution timed out|receipts:\s*vec!\[\]" agent/features/runtime/src
```

预期：第一条通过；第二条无生产命中。

- [ ] **步骤 4：运行生产可达性**

```bash
cargo run -p xtask -- production-reachability .
```

- [ ] **步骤 5：提交退役**

```bash
git add agent/features/runtime agent/features/tools/src/adapters .agents/hooks/check-runtime-tool-execution-supervisor.sh
git commit -m "refactor: #1440 retire tool timeout bypasses"
```

### Task 15：完整验证与 L5 smoke

**文件：**
- 修改：`docs/design/03-engineering/03-migration-governance.md`（回写证据）
- 修改：Issue #1440 / PR Test plan（执行阶段通过 `gh` 更新，不在本计划阶段操作）

- [ ] **步骤 1：环境与格式门禁**

```bash
scripts/setup-dev-env.sh --check
cargo fmt --all -- --check
git diff --check
```

- [ ] **步骤 2：crate 定向测试**

```bash
cargo test -p tools --all-targets
cargo test -p context --all-targets
cargo test -p storage --all-targets
cargo test -p runtime --all-targets
cargo test -p cli archify_glob_running_survives_forced_abort -- --nocapture
```

- [ ] **步骤 3：生产与架构门禁**

```bash
cargo check --workspace
cargo run -p xtask -- production-reachability .
cargo clippy --workspace --all-targets -- -D warnings
.agents/hooks/check-architecture-guards.sh --full
```

- [ ] **步骤 4：workspace 回归与覆盖率**

```bash
cargo test --workspace
./scripts/coverage.sh
```

保留首次结果；失败不得用重跑成功覆盖。记录 tools/runtime/context/storage 的 line/region/function 和 changed-lines 信号，但不以百分比替代行为证据。

- [ ] **步骤 5：真实 TUI L5 smoke**

在隔离 HOME/agents dir 下构建并运行 TUI，分别执行：慢 Glob `**/archify.mjs`、派生后台子进程的长 Bash、可控 MCP stub。测试配置可缩短 deadline，但必须复用生产 supervisor。验证：deadline/cancel 后 UI 可继续输入；Bash 进程树不存在；MCP/Agent 终态真实；重启 session 仍能看到 call；`agent-runtime.log` 可按 identity 串联且无完整敏感 input。

- [ ] **步骤 6：回写矩阵和 Issue 门禁**

在 migration governance 与 #1440 checklist 逐项标记：实现路径、测试名、命令、结果；不能完成或不适用项写明证据、影响和 owner。不得在有未解释 check 时创建 PR 或宣称完成。

- [ ] **步骤 7：最终提交**

```bash
git add docs/design/03-engineering/03-migration-governance.md
git commit -m "docs: #1440 record tool deadline verification"
```

---

## 验收追踪表

| Issue 门禁 | 主要任务 | 核心证据 |
|---|---:|---|
| 所有入口唯一 supervisor | 5–7、13–14 | paused-time L2 + 零旁路 guard |
| 120 秒墙钟硬截止 | 5、8–10 | timer 可调度、阻塞隔离、typed terminal |
| cooperative/unconfirmed 真实性 | 5、9–10 | cleanup ack、process reap、remote/child terminal |
| 调用不从 Session 消失 | 3–4、11–12 | mutation contract、v4 reopen、forced abort |
| Main/Sub/Agent/MCP/approval 同 taxonomy | 6–7、10、12 | L3/L4 统一场景 |
| provider 消息完整且不伪造成功 | 11 | restore tests |
| 生命周期日志可关联且脱敏 | 13 | log capture tests + target guard |
| 文档—代码双向一致 | 1、15 | migration matrix + Issue checklist |

## 非目标与风险控制

- 不恢复或重放 Run future，不承诺 exactly-once。
- 不把 timeout/semaphore/policy/hook 下沉到 Tools `ExecutionAdapter`。
- 不创建 Runtime 第二份 Session backing，不恢复 loop-exit `save_chain`。
- 不把 `spawn_blocking` 的 future drop 误报为底层任务已停止。
- 不因 #1440 改变 MCP Ready/catalog revision 生命周期；只修当前调用的 cancel 结果映射。
- 不用短 sleep 证明 deadline、互斥或进程退出；虚拟时间和条件等待是业务证据。
- 若实施中确认必须拆成多个独立 PR，先向用户申请 GitHub 原生 sub-issues；未经同意仍按单 Issue/单 PR 计划执行。
