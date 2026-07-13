# Issue #700 共享 Loop Engine 与 Run 状态机续作计划

> **执行要求**：后续会话必须使用 `superpowers:executing-plans` 或 `superpowers:subagent-driven-development` 按任务顺序执行；每个核心逻辑任务遵循 Red → Green → Refactor。

**日期**：2026-07-12

**对应 Issue**：[#700](https://github.com/rushsinging/aemeath/issues/700)

**伞 Issue**：[#743](https://github.com/rushsinging/aemeath/issues/743)

**前置设计**：[#761](https://github.com/rushsinging/aemeath/issues/761) / PR #785

**目标分支**：`release/v0.1.0`

**工作分支**：`feat/700-shared-run-loop`
**状态**：执行中；本文记录 2026-07-12 checkpoint 后的唯一续作顺序。

## 1. 目标

把当前 Main Run 与 Sub Run 的两套执行过程收敛为：

```text
RunSpec → RuntimeContext → Run → shared Loop Engine
```

最终满足：

- `Run` 是全系统唯一领域状态机，Session 只负责管理 Run 序列；
- Main/Sub 复用同一个生产 Loop Engine，差异只由规格、上下文和 adapter 表达；
- Sub Run 显式关联父 Run，父取消向下传播，子取消不反向传播；
- user message 与 control command 分流；
- `cancel_run(run_id)` 是唯一同步取消入口，`RunCancelled` 是异步收口 ACK；
- Provider、Tool、Compact、Hook 监听同一 Run cancellation scope 或其派生 scope；
- 不使用 `max_turns`，停止兜底统一为 timeout + StuckGuard；
- 旧 Main FSM、旧 Sub loop、Session 级可替换 token 槽和取消双入口退出生产路径。

## 2. 设计真相与范围边界

实施时按以下优先级判定：

1. `docs/design/02-modules/runtime/01-domain-model.md`
2. `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
3. `docs/design/02-modules/runtime/06-ports-and-adapters.md`
4. `docs/design/03-engineering/migration-governance.md`
5. Issue #700 验收清单

本 Issue 允许用薄 adapter 包装现有依赖，但不提前完成 #649 的最终物理模块迁移，也不抽取其他 BC 尚未定案的全部端口。SDK/TUI 的完整 Published Language 和状态投影分别由 #612/#742/#797 接续；本 Issue 只完成取消单入口和必要 Run 事件契约。

## 3. Checkpoint 基线

### 3.1 已形成的实现

截至本文创建时，worktree 中已有以下未提交实现：

- `business/agent_run`：`Run`、`RunSpec`、`RunStatus`、`RunStep`、领域事件和两阶段取消；
- `business/loop_engine`：共享 `run_loop`、输入分流值对象、StuckGuard；
- Main adapter：`chat/looping/main_run_port.rs`；
- Sub adapter：`agent/runner/loop_run.rs` 已调用共享引擎；
- `core/active_run.rs`：同步 `cancel_run(run_id)` 原型；
- SDK：`RunId`、`CancelRunOutcome`、Run 生命周期事件；
- TUI：Esc/Ctrl+C 统一产生 `Effect::CancelCurrentRun`；
- Provider、Tool、Compact、Hook：已开始传递 `CancellationToken`；
- 旧 `chat/looping/state.rs` 已退出当前实现；
- 已增加 Run FSM、共享引擎、取消和 TUI effect 的部分测试。

### 3.2 尚未满足的关键不变量

- Sub 生产路径仍以 `parent_run_id = None` 创建 Run；
- `RunSpec` 目前只有 `name + timeout`，不能表达 Main/Sub 资源与交互策略；
- `RunStep` 尚未持有单次 Model Invocation 和 Tool Call 实体；
- Active Run registry 仍是单槽，不能表达并发父子 Run；
- 全部合法/非法状态迁移尚未矩阵化覆盖；
- Sub 终态事件尚未通过父事件出口回传；
- 各异步层对同一 scope 的监听尚未逐层验收；
- 工作分支落后 `release/v0.1.0` 3 个提交，其中 #801 已修改 Agent tool 的 `model/task_id` 契约，存在冲突风险；
- 尚未取得 workspace check/test/clippy 的通过证据。

## 4. 执行原则

- 每个任务只交付一个可验证结果；不得把“实现并验证”合成一个任务。
- 修改核心逻辑前先写失败测试，保留失败输出，再写最小实现。
- 跨 SDK → Runtime → Tools → TUI 的链路，每层必须有测试，不能只测首尾。
- 每完成一个阶段先运行定向门禁；全量门禁只在所有阶段结束后运行。
- checkpoint commit 只用于保存当前可编译中间态；不得把它描述成 Issue 完成。
- 同步 `release/v0.1.0` 后，优先保留 release 上已合入的 #801 Published Language，禁止把 `model/task_id` 重新引入 Agent tool。

## 5. 分阶段任务

### Phase 0：保存并校准当前 checkpoint

#### Task 0.1：落盘本续作计划

**文件**：
- 新建：`docs/superpowers/plans/2026-07-12-issue-700-shared-run-loop.md`

**验收**：
- 文档包含当前基线、剩余缺口、逐步 TDD 任务、验证门禁和退役清单。

#### Task 0.2：格式检查当前 checkpoint

**命令**：

```bash
cargo fmt --check
```

**验收**：命令成功；若失败，只运行 `cargo fmt` 修复机械格式，不调整逻辑。

#### Task 0.3：编译当前 checkpoint

**命令**：

```bash
cargo check --workspace
```

**验收**：命令成功；若失败，仅修复使当前已写实现恢复编译所需的问题。

#### Task 0.4：运行当前 checkpoint 定向测试

**命令**：

```bash
cargo test -p runtime business::agent_run
cargo test -p runtime business::loop_engine
cargo test -p sdk
cargo test -p cli tui::app::update::key
```

**验收**：四组定向测试通过。

#### Task 0.5：提交 checkpoint

**范围**：当前 Issue #700 的设计 commit 后全部实现、本计划及必要 lockfile/guard 更新。

**验收**：
- commit 标题明确 `#700` 和共享 Loop/Run checkpoint，不宣称完成；
- `git status --short` 为空；
- commit 文件清单不包含 `target/` 或无关运行时产物。

### Phase 1：同步集成分支并消除接口漂移

#### Task 1.1：拉取最新 `release/v0.1.0`

**命令**：

```bash
git pull origin release/v0.1.0
```

**验收**：工作分支包含 release 最新提交；冲突文件清单已记录。

#### Task 1.2：对齐 #801 Agent tool 契约

**文件**：
- `agent/shared/src/tool/types/agent.rs`
- `agent/features/tools/src/business/agent_tool.rs`
- `agent/features/tools/src/contract/agent_port.rs`
- `agent/features/runtime/src/business/agent/runner/setup.rs`

**动作**：保留 release 上“Agent tool 去掉 model 和 task_id 参数”的契约；只迁移 #700 所需 timeout、父 Run 和终态语义。

**验收**：

```bash
cargo test -p tools agent_tool
cargo check -p runtime
```

### Phase 2：补齐 Run 聚合不变量

#### Task 2.1：为全部状态迁移建立表驱动测试

**文件**：
- 修改：`agent/features/runtime/src/business/agent_run/tests.rs`

**动作**：为每个 `RunStatus × RunTransition` 组合断言合法目标态或 `IllegalTransition`，并覆盖任意活跃态取消、Cancelling 只允许收口、终态拒绝新工作。

**验收**：新增测试在现有实现上先失败，失败点对应缺失迁移而非测试错误。

#### Task 2.2：补齐 Run FSM 迁移

**文件**：
- 修改：`agent/features/runtime/src/business/agent_run/domain.rs`

**动作**：按设计迁移表补齐 retry、context exceeded → compact、取消优先级和终态拒绝；错误消息改为中文并遵循项目错误体系边界。

**验收**：Task 2.1 表驱动测试全部通过。

#### Task 2.3：为 RunStep/ModelInvocation/ToolCall 不变量写失败测试

**文件**：
- 修改：`agent/features/runtime/src/business/agent_run/tests.rs`

**断言**：
- 每个 RunStep 至多接受一次 Model Invocation；
- 每个 Tool Call 必须由 RunStep 创建并归属该 Step；
- Tool Call 状态只能单向推进；
- terminal/Cancelling Run 不可增加 invocation 或 tool call。

**验收**：测试因领域 API/不变量缺失而失败。

#### Task 2.4：实现 RunStep 内部实体和值对象

**文件**：
- 修改：`agent/features/runtime/src/business/agent_run/domain.rs`
- 必要时拆分：`agent/features/runtime/src/business/agent_run/step.rs`

**动作**：让 Run 聚合独占 RunStep、ModelInvocation 和 ToolCall 生命周期；复用现有 ID/结果类型，禁止复制 Tool Call 业务结构。

**验收**：Task 2.3 测试通过，且单文件保持职责清晰。

### Phase 3：用 RunSpec 表达 Main/Sub 策略

#### Task 3.1：为 RunSpec 模式与派生约束写失败测试

**文件**：
- 修改：`agent/features/runtime/src/business/agent_run/tests.rs`

**断言**：
- Main 默认 timeout=0、共享上下文、可交互、事件出口到 TUI；
- Sub 使用隔离上下文、受限工具、父事件出口和可配置 timeout；
- Sub 派生只能收缩工具/权限，不能放宽；
- 不存在 `max_turns`。

#### Task 3.2：扩展 RunSpec 最小 S3 模型

**文件**：
- 修改：`agent/features/runtime/src/business/agent_run/domain.rs`

**动作**：增加 RunKind、输入模式、交互能力、事件出口和 S3 必需资源模式；尚未定案的 BC 依赖只保留领域枚举，不在本 Issue 抽最终 12 个 Port。

**验收**：Task 3.1 测试通过。

#### Task 3.3：让 Main/Sub adapter 只读取 RunSpec/RuntimeContext 差异

**文件**：
- 修改：`agent/features/runtime/src/business/chat/looping/loop_runner.rs`
- 修改：`agent/features/runtime/src/business/chat/looping/main_run_port.rs`
- 修改：`agent/features/runtime/src/business/agent/runner/loop_run.rs`
- 修改：`agent/features/runtime/src/business/agent/runner/setup.rs`

**验收**：共享 `run_loop` 中不出现 Main/Sub 类型分支；两个 adapter 的行为由 spec/context 提供。

### Phase 4：接通父子 Run 与终态事件

#### Task 4.1：为父子 ID 和 cancellation tree 写失败测试

**文件**：
- 修改：`agent/features/runtime/src/business/agent/runner/tests.rs`
- 修改：`agent/features/runtime/src/business/loop_engine/tests.rs`

**断言**：父 Run ID 进入 Sub Run；父 token 取消子 token；子 token 取消不影响父；Sub 所有领域事件携带 parent_run_id。

#### Task 4.2：把父 Run 身份传入 ToolExecutionContext

**文件**：
- 修改：`agent/features/tools/src/contract/context.rs`（以实际定义路径为准）
- 修改：Main tool context 构造点
- 修改：Sub tool context 构造点

**动作**：增加只读 `run_id` 和派生 scope；不得让 Tools 依赖 Runtime 的 Run 聚合。

**验收**：Tools crate 编译，父子传播测试通过到 context 层。

#### Task 4.3：以父 ID 创建 Sub Run

**文件**：
- 修改：`agent/features/runtime/src/business/agent/runner/loop_run.rs`

**验收**：Sub `Run::new` 不再传 `None`，Task 4.1 的父 ID 断言通过。

#### Task 4.4：建立 Sub 终态事件出口

**文件**：
- 修改：`agent/features/runtime/src/business/agent/runner/loop_run.rs`
- 修改：`agent/features/tools/src/contract/agent_port.rs`
- 修改：父 Run tool result adapter

**动作**：Sub 成功/失败/取消统一由 `RunCompleted{result}` / `RunFailed{error}` / `RunCancelled` 产生；父 adapter 从终态事件得到结果，禁止遍历 message 推断。

**验收**：三种 Sub 终态均有父层测试，且只出现一次终态投影。

### Phase 5：完成 per-Run 取消注册与逐层传播

#### Task 5.1：为多 Run registry 写失败测试

**文件**：
- 修改：`agent/features/runtime/src/core/active_run.rs`

**断言**：可同时注册父 Run 和多个 Sub Run；按 ID 取消；重复取消幂等；终态竞争遵循“先接受取消则取消胜出”；clear 只删除目标 Run。

#### Task 5.2：把 ActiveRunRegistry 从单槽改为按 RunId 存储

**文件**：
- 修改：`agent/features/runtime/src/core/active_run.rs`
- 修改：`agent/features/runtime/src/business/agent_run.rs`

**验收**：Task 5.1 测试通过；不存在 Session 级可替换 token 槽。

#### Task 5.3：注册并清理 Main/Sub Run

**文件**：
- 修改：`agent/features/runtime/src/business/chat/looping/loop_runner.rs`
- 修改：`agent/features/runtime/src/business/agent/runner/loop_run.rs`

**验收**：正常、失败、取消、panic-safe 收口路径都不会遗留 active Run。

#### Task 5.4：验证 Provider 使用 Run scope

**文件**：
- 修改测试：`agent/features/provider/src/core/pool.rs` 或对应 provider 测试模块

**验收**：取消 scope 后在途 provider future 结束，且映射为取消而非普通 API error。

#### Task 5.5：验证 Tool 使用 Run 派生 scope

**文件**：
- 修改测试：`agent/features/tools/src/business/agent_tool_tests.rs`
- 修改测试：Runtime tool coordination 测试

**验收**：取消后不再启动新 Tool Call，活动 Tool 收到派生 scope。

#### Task 5.6：验证 Compact 使用 Run scope

**文件**：
- 修改测试：`agent/features/runtime/src/business/compact/summary.rs`
- 修改测试：`agent/features/runtime/src/business/chat/looping/compact.rs`

**验收**：compact 不创建脱离 Run 的独立根 token；取消时回滚并进入 Cancelled。

#### Task 5.7：验证 Hook 使用 Run scope

**文件**：
- 修改测试：`agent/features/hook/src/business/hook/runner.rs`
- 修改测试：Runtime hook adapter 测试

**验收**：Hook future 响应取消；Cancelling 后不启动新 Hook。

### Phase 6：收敛输入和 TUI 唯一取消路径

#### Task 6.1：为 user/control 分流写跨层测试

**文件**：
- 修改：`agent/features/runtime/src/business/loop_engine/tests.rs`
- 修改：Runtime input adapter 测试

**断言**：user message 进入 Context；control command 不进入 Context；idle 阻塞等待；busy 只做非阻塞 drain。

#### Task 6.2：将生产输入接到分流模型

**文件**：
- 修改：`agent/features/runtime/src/business/chat/looping/idle_lifecycle.rs`
- 修改：`agent/features/runtime/src/business/chat/looping/input_gate.rs`
- 修改：`agent/features/runtime/src/business/chat/looping/main_run_port.rs`

**验收**：生产路径使用 `RuntimeInputBatch`/等价单一模型，不再靠 idle/gate 两套控制语义。

#### Task 6.3：验证 Esc/Ctrl+C 单一 Effect

**文件**：
- 修改：`apps/cli/src/tui/app/update/key_tests.rs`
- 修改：`apps/cli/src/tui/effect/session/processing.rs` 测试模块

**断言**：Esc 和首次 Ctrl+C 各只产生一个 `CancelCurrentRun`；Effect 只调用一次 `cancel_run(run_id)`；收到 `RunCancelled` 前保持 Cancelling。

#### Task 6.4：退役旧取消入口

**文件**：
- 修改：`packages/sdk/src/chat.rs`
- 修改：Runtime input event conversion
- 修改：相关测试和 guard

**动作**：删除或仅保留映射到同一个 `cancel_run(active_run_id)` 的迁移壳；不得形成第二套 Runtime 取消语义。

**验收**：全仓搜索没有生产代码直接消费 `ChatInputEvent::Cancel`。

### Phase 7：退役旧循环并锁定架构

#### Task 7.1：证明 Main/Sub 都只调用共享引擎

**文件**：
- 修改：Runtime 架构测试或 guard 脚本

**验收**：Main 和 Sub 各只有 adapter，均调用唯一 `business::loop_engine::run_loop`；无第二套生产模型/tool while-loop。

#### Task 7.2：清理旧 FSM、token 槽和兼容 helper

**文件**：以全仓引用搜索结果为准。

**验收**：
- `chat/looping/state.rs` 不存在；
- Session 级 token replace/reset 逻辑不存在；
- 仅测试引用或已被替代的 helper 已删除；
- 对暂留薄 adapter 标注 #649 退役点。

#### Task 7.3：更新架构守卫说明并验证守卫

**文件**：
- 修改：`.agents/hooks/check-agent-client-trait-minimal.sh`
- 修改：`docs/design/02-architecture-guards.md`

**验收**：故意引入第二个 AgentClient 控制方法或旧取消入口时 guard 失败，恢复后 guard 通过。

### Phase 8：完整验证与 Issue 同步

#### Task 8.1：格式门禁

```bash
cargo fmt --check
```

#### Task 8.2：定向测试门禁

```bash
cargo test -p sdk
cargo test -p provider
cargo test -p tools
cargo test -p hook
cargo test -p runtime
cargo test -p cli
```

#### Task 8.3：workspace 测试门禁

```bash
cargo test --workspace
```

#### Task 8.4：workspace clippy 门禁

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

#### Task 8.5：死代码与双路径扫描

```bash
rg "ChatLoopState|ChatInputEvent::Cancel|max_turns|Mutex<CancellationToken>|run_loop" agent packages apps
```

**验收**：仅保留设计允许项；所有异常命中逐项解释或清理。

#### Task 8.6：同步追踪状态

**动作**：
- 更新 Issue #700 当前状态和验收 checklist；
- 更新伞 Issue #743 S3 checklist；
- 更新 Release Gate #579 的关联项和新增验收行为；
- 创建 PR 前执行 `git pull origin release/v0.1.0` 并重新跑全量门禁。

**约束**：不自行关闭 Issue，不自行合并 PR。

## 6. 阶段依赖

```text
Phase 0 checkpoint
  → Phase 1 release 对齐
    → Phase 2 Run 聚合不变量
      → Phase 3 RunSpec 策略
        → Phase 4 父子 Run/终态事件
          → Phase 5 per-Run 取消树
            → Phase 6 输入/TUI 单入口
              → Phase 7 退役与 guard
                → Phase 8 全量验证
```

除各 crate 的独立取消测试外，主链严格串行；任何阶段失败都先修复当前阶段，不跨阶段堆积更多未验证改动。

## 7. 风险与控制

1. **当前 diff 很大**：先建立可编译 checkpoint，再同步 release，避免冲突时丢失工作。
2. **#801 契约冲突**：release 版本优先，#700 只迁移必要的 Run/cancel 字段。
3. **父子取消竞态**：用 registry 终态 claim + cancellation token 先后顺序测试锁定，禁止依赖时序猜测。
4. **事件重复终态**：Run 聚合是唯一终态事件生产者；adapter 只能投影，不能自行构造第二个终态。
5. **跨层取消遗漏**：Provider/Tool/Compact/Hook 分层测试，不能只做 Loop Engine mock 测试。
6. **RunSpec 过度扩张**：S3 只落领域模式和薄装配；最终 12 Port 与物理目录迁移留给 #649。
7. **Main/Sub 表面复用、内部复制**：guard 同时检查调用入口和旧生产 loop，不只检查函数名。

## 8. 完成定义

只有同时满足以下条件，Issue #700 才进入待 review：

- Main/Sub 共用唯一生产 Loop Engine；
- Run 聚合守护状态、Step、Invocation、Tool Call 和终态不变量；
- RunSpec/RuntimeContext 表达 Main/Sub 差异，引擎零分支；
- 父子 ID、终态事件和取消传播完整；
- `cancel_run(run_id)` 是唯一同步入口；
- Provider/Tool/Compact/Hook 逐层取消测试通过；
- 旧 FSM、旧 Sub loop、Session token 槽和取消双入口退出生产路径；
- `cargo test --workspace` 通过；
- `cargo clippy --workspace --all-targets -- -D warnings` 通过；
- Issue #700、#743、#579 状态已同步；
- PR 以 `release/v0.1.0` 为 base 创建，等待用户 review，agent 不自动合并。
