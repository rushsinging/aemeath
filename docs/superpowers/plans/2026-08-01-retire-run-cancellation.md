# 旧 Run cancellation 与第二生命周期源退役实施计划

## 目标

将运行控制收敛为两条唯一协议：当前 Step 取消后回到输入 drain，Run 终止后进入 `Terminated`。物理删除旧 Run 级 cancellation 状态机、registry 生命周期副本、SDK/TUI 兼容事件，并让 TUI 活动态只从 Runtime 权威事实派生。

## 范围与边界

- 纳入：Runtime Domain、shared loop、ActiveRunRegistry、SDK Published Language 与 mapper、TUI adapter/model/view、Runtime→Tools progress/plan 所有权、目标 Guard、Target/Migration 文档。
- 不纳入：`RuntimeResources`、`ChatLoopContext`、fat `RunLoopPort`、Main/Sub adapter 物理退役；TUI slash I/O 全 Effect 化；Task BC 事件模型重构。
- 术语：Run 终态只有 `Completed / Failed / Terminated`；`Cancelled` 只允许用于 Step、Tool、Interaction 等局部执行结果。

## 实施清单

### 1. 同步基线并冻结退役契约

- [x] 基于最新 `origin/main` 更新 worktree。
- [x] 回填 Issue 开发前文档差异与 Guard 白名单预算。
- [ ] 为 Runtime Domain 建立测试，断言 Run 不存在 cancellation 状态/转换且 Step cancel 后进入 drain。
- [ ] 为 SDK Published Language 建立测试，断言只发布 current-step cancellation 与 terminate 命令/终态。

验证：定向运行 Runtime Domain 与 SDK 契约测试，先观察旧结构导致的失败。

### 2. 退役 Run 级 cancellation Domain 与 shared loop

- [ ] 删除 `RunStatus::Cancelling / Cancelled`。
- [ ] 删除 `RunTransition::CancellationFinished`、对应 transition reason、`RunCancellationRequest`。
- [ ] 删除 `RunDomainEvent::CancellationRequested / Cancelled`。
- [ ] 删除 `Run::request_cancellation / finish_cancellation`。
- [ ] 把 root cancellation token 与旧 cancel 分支映射到 typed `CancelRunStep` 或 `TerminateRun`，禁止产生第二 Run 终态。
- [ ] 保证 pending Interaction 在 Step cancel/terminate 时由 Runtime 清理，局部原因保持 typed。

验证：Domain 状态机、loop engine、interaction coordinator 的相邻测试通过；取消后存在 Step cancel/drain 或 Run terminated，且不存在成功/旧 cancelled 冲突终态。

### 3. 收窄 ActiveRunRegistry 与控制端口

- [ ] 删除 `ActiveRun.cancelling / terminal` 生命周期副本。
- [ ] 删除 `ActiveRunRegistry::cancel` 与 `CancelRunOutcome`。
- [ ] 删除 registry 的 `claim_cancellation / claim_terminal` 仲裁职责；终态唯一性由 Run/Step 聚合与单一 finalization owner 保证。
- [ ] 让 `cancel_current_main` 无活动 Step 时返回窄 typed outcome，不再通过 root token启动旧 Run cancellation。
- [ ] 保留定位、root/step cancellation scope、typed control command 与 clear 职责。

验证：Registry 测试覆盖不存在、无 active Step、重复 Step cancel、terminate 优先级与 clear；不复制聚合生命周期。

### 4. 退役 SDK 与 Runtime adapter 兼容投影

- [ ] 删除 `CancelRunOutcome` 的 export、wire schema 注册与调用点。
- [ ] 删除 `ChatEvent::RunCancelling / RunCancelled` 及 RuntimeStreamEvent 对应变体。
- [ ] 删除 `RunDomainEvent` 到旧 ChatEvent 的 mapper 分支。
- [ ] 删除 no-TUI/TUI event mapping、日志、Interaction command 中依赖旧 RunCancelling 的兼容处理；改用明确的 current-step/terminate 结果。
- [ ] 更新并生成 SDK wire schema（如 schema check 要求）。

验证：Runtime mapper、SDK contract、SDK→TUI adapter 各层测试通过，静态扫描无旧 PL 符号。

### 5. 删除 TUI 第二生命周期源

- [ ] 先建立 reducer/view 场景测试：processing、spinner、tool 活动态从 Conversation 中的 Run/Step/Tool/Compact/Hook facts 派生。
- [ ] 删除 Run projection 中旧 `Cancelling / Cancelled` 状态与 intent/change/reducer 分支。
- [ ] 删除 `SpinnerModel.chat_active`、`running_tool_count` 和存储式 `phase` 写路径。
- [ ] 将 `SpinnerPhase` 改为 view assembler 的纯派生值；动画 frame 仍归 view state。
- [ ] 删除 tool counter 增减、Start/CompleteChat 平行布尔写入与 stream teardown 推断终态。

验证：TUI event→intent、intent→change、reducer→view、最终场景逐层测试；一个 turn 最多一个终态，Step cancel 后仍可继续下一 Step。

### 6. 收回 Runtime-owned progress 与 plan 资源

- [ ] 先建立 Runtime→Tools 契约测试，断言 Tools execution context 不持有 Runtime progress/plan 生命周期状态。
- [ ] 将 Bash streaming progress 和 Agent progress 通过 Runtime invocation/supervisor 窄端口协调。
- [ ] 将 PlanMode 状态读取/变更收敛到 Runtime-owned Run context 或独立窄命令，不让 `ToolExecutionContext` 持有 `PlanModeState`。
- [ ] 删除 Tools `ProgressSink / PlanModeState / with_progress` 生产导出与 context 字段。

验证：Tools domain/context、Bash streaming、Agent progress、PlanMode adapter、Runtime tool supervisor 相邻测试与场景测试。

### 7. 固化 Guard 并更新治理文档

- [ ] 扩充 Runtime capability Guard：禁止旧 Run cancellation Domain/API/Event、registry 生命周期副本与旧 SDK PL 复活。
- [ ] 新增或扩充 TUI lifecycle Guard：禁止 `chat_active`、tool counter、存储式 spinner phase 和旧 Run cancel projection。
- [ ] 结构化检查唯一合法入口，不新增路径白名单或 `grep -v`。
- [ ] 删除 stale migration exception，并同步 Guard Registry、Migration Governance、Runtime/TUI Target 文档。
- [ ] 对每条新规则制造违规，确认单 Guard 与总编排均 exit 2；恢复后 clean pass。

### 8. 完整验证与 Issue 收尾

- [ ] `cargo fmt --all -- --check`。
- [ ] Runtime、SDK、CLI 定向测试。
- [ ] `cargo test --workspace`。
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`。
- [ ] fast/full architecture guards 与 Stop Hook。
- [ ] 静态扫描确认旧符号、旧投影与第二状态源为零。
- [ ] 回填 Issue 差异处置、白名单完成预算、验证证据与全部 checklist。

## 风险控制

- 不把 Step/Tool/Interaction 的局部 `Cancelled` 一并删除；扫描与 Guard 必须限定 Run 领域语义。
- 不把 root token 的任意取消机械映射为成功 Step cancel；无活动 Step 时必须由 typed control outcome 或 terminate 协议明确处理。
- TUI 不通过 stream close 或缺失事件猜测终态；只有 Runtime Published Language 的权威业务事实可结束 processing。
- Runtime→Tools 所有权调整涉及独立 BC；若审计发现必须改变 Tools 对外行为而非仅收窄资源边界，先回填 Issue 差异再继续。
