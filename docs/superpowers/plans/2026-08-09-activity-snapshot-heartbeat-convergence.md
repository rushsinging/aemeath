# Runtime Activity Snapshot 心跳收敛实施计划

> **范围：** Issues #945 / #1502、PR #1541。将 Runtime Activity 从碎片化增量生产收敛为 logical-commit 完整 Snapshot，并复用固定心跳刷新权威 timing sample；TUI 原子替换 Activity facts。
> **执行方式：** 在 `feat/945-1502-control-terminal-convergence` worktree 中严格 TDD，逐层验证 Runtime → SDK → TUI ACL → Model → ViewState → LiveStatus。PR 保持 OPEN，禁止 merge/close。

## 1. 已验证根因

实机日志表明，同一 Main root Activity 的 identity 始终稳定，最终 Runtime total 正确；问题来自旧 primary `Finished` 与新 primary `Started` 分两条增量发布。TUI 在两条消息间观察到“root live、primary absent”，`ActivitySummaryAssembler` 返回 `None`，继而清空 outer baseline 并让整个 Spinner 消失。

仅把每条 mutation 包装成 Snapshot 无法解决问题；发布边界必须从 registry mutation 提升为 logical transition commit。

## 2. 目标契约

1. Runtime production 只发布 `ActivitySnapshot`，不再发布 `ActivityChanged`。
2. SDK public `ChatEvent::ActivityChanged` 暂时保留为 compatibility ingress；TUI 第一层 ACL 可继续接收，但 canonical production reference 必须为零。
3. `ActivitySnapshotView` 使用双序号：
   - `revision`：Activity graph business revision，只在 logical commit 后递增；
   - `heartbeat_sequence`：同一 business revision 下每次心跳递增。
4. Snapshot 构造只采样一次 Runtime 单调时钟；同一 Snapshot 中 root total 与 primary state elapsed 来自同一 observation time。
5. logical Run transition 原子完成旧 phase terminal、新 phase start、root/leaf 更新后，只提交一个 Snapshot；TUI 永远不观察内部中间 graph。
6. business commit dirty 后立即发布 Snapshot；运行期间每秒心跳重发 Snapshot，刷新 Runtime 权威 timing sample。
7. TUI 按 `(run_id, revision, heartbeat_sequence)` 原子替换：高 revision 接受；同 revision 仅接受更高 heartbeat；stale/duplicate 拒绝。
8. root 与 primary 展示解耦：live root 足以保留 outer；primary 缺失时只隐藏 inner/phase；root terminal 后整个 Spinner 才消失。
9. Snapshot payload 有界：保留 live graph 与当前消费必要的 terminal Activity；不得随长 Run 无界增长。若现有 detail/history 消费要求暂时保留 terminal，则必须增加容量测试并在后续清理项中明确。

## 3. 实施任务

### Task 1：定义 SDK Snapshot 双序号

**修改：**
- `packages/sdk/src/activity.rs`
- `packages/sdk/src/activity_tests.rs`
- `packages/sdk/schema/wire-components.schema.json`（由 xtask 更新）

**测试先行：**
- Snapshot 序列化/反序列化携带 `heartbeat_sequence`。
- 相同 business revision 可以拥有递增 heartbeat。

**实现：**
- 为 `ActivitySnapshotView` 增加 `heartbeat_sequence: u64`。
- 不改 `ActivityView.revision`；它继续表示单 Activity 最后一次变更序号。

### Task 2：建立 Activity logical commit 与 Snapshot 构造

**修改：**
- `agent/features/runtime/src/application/activity/coordinator.rs`
- `agent/features/runtime/src/application/activity/coordinator_tests.rs`
- 必要时新增同级职责文件与 tests 文件，避免 coordinator 继续膨胀。

**测试先行：**
- 多个内部 mutation 在一次 transaction 中只增加一次 graph business revision。
- 同一 commit 后只发布一个 Snapshot，不发布 `Changed`。
- Snapshot 全部 timing 使用同一固定 clock sample。
- heartbeat 保持 revision、递增 heartbeat_sequence，并重新采样 root total。

**实现：**
- 将 registry mutation 与 publication 分离。
- 增加 transaction/commit 边界，内部 start/update/finish 可静默 mutation。
- commit 后构造完整 Snapshot、heartbeat_sequence 归零并 dirty-immediate 发布。
- heartbeat 从 coordinator 当前 registry 构造新 timing Snapshot，不改变 business revision。

### Task 3：原子化 Run phase transition

**修改：**
- `agent/features/runtime/src/application/activity/run_events.rs`
- `agent/features/runtime/src/application/activity/run_events_tests.rs`

**测试先行：**
- `old phase Finished → new phase Started` 对外只有一个 Snapshot。
- 已发布 Snapshot 始终包含 live root；非 terminal transition 后包含新 primary。
- root total 跨多次 transition 单调，new primary state elapsed 从零开始。
- terminal transition 对外只发布 root terminal 后的单一 Snapshot。

**实现：**
- `observe_run_events` 以领域 event batch 为 logical commit。
- `observe_run_transition` 内 finish-old/start-new 不单独发布。
- batch 完成后一次 commit/publish。

### Task 4：将其他 Activity producer 切到 commit Snapshot

**修改：**
- `agent/features/runtime/src/application/activity/model_tool.rs`
- `agent/features/runtime/src/application/activity/runtime_work.rs`
- compaction/hook/model/tool 调用点及相邻测试。

**测试先行：**
- 单 producer mutation 完成后发布一个 Snapshot。
- 组合 mutation 不暴露中间 graph。
- terminal Activity 不残留 live graph。

**实现：**
- 公共 producer helper 使用 logical transaction API。
- 禁止从 `start/update/finish/transition` 内直接 `publish_change`。

### Task 5：接入固定心跳生命周期

**修改：**
- `agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs`
- `agent/features/runtime/src/application/run/context.rs`
- 相邻 session driver tests。

**测试先行：**
- 每个 active Run 的 heartbeat tick 发布 ActivitySnapshot。
- cancel/Run 结束后 heartbeat task 停止，无 detached task。
- Runtime Status heartbeat 与 Activity heartbeat 共用 session/run task 生命周期，但 family revision 相互独立。

**实现：**
- 复用现有每秒 heartbeat task；同一 tick 分别取 status heartbeat 与 active Run Activity heartbeat。
- Activity heartbeat 不塞进 `PublishedStateRegistry.status` backing；复用 heartbeat 调度基础设施，保持 Activity registry 为唯一 graph/timing source。

### Task 6：收敛 Runtime/SDK producer 事件

**修改：**
- `agent/features/runtime/src/application/loop_engine/chat/events.rs`
- `agent/features/runtime/src/adapters/sdk_event_sink.rs`
- `agent/features/runtime/src/adapters/sdk_event_mapper.rs`
- 对应 mapper/sink tests。

**测试先行：**
- production sink 只映射 Snapshot。
- SDK public `ActivityChanged` variant 仍可构造，不由 Runtime production 枚举生产。

**实现：**
- `RuntimeActivityEvent` production 收敛为 Snapshot。
- `ActivityChangePublisher::publish_change` 与实现退役。

### Task 7：TUI Snapshot 双序号原子消费

**修改：**
- `apps/cli/src/tui/adapter/tui_runtime_event.rs`
- `apps/cli/src/tui/adapter/event_mapping.rs`
- `apps/cli/src/tui/model/conversation/activity_observation.rs`
- `apps/cli/src/tui/model/conversation/model.rs`
- 对应 adapter/model tests。

**测试先行：**
- heartbeat_sequence 无损映射。
- 高 business revision 接受。
- 同 revision、更高 heartbeat 接受并原子替换 timing。
- stale/duplicate heartbeat 拒绝。
- Snapshot replacement 不出现 transient primary gap。
- legacy ActivityChanged 只在 first ACL compatibility 路径归一化，不建立第二 canonical mirror 规则。

**实现：**
- mirror identity 改为 `(revision, heartbeat_sequence)`。
- Snapshot 成为 canonical replacement。
- 增量 compatibility 路径明确隔离并记录兼容日志。

### Task 8：解耦 root outer 与 optional primary inner

**修改：**
- `apps/cli/src/tui/view_assembler/activity_summary.rs`
- `apps/cli/src/tui/view_state/run_activity.rs`
- `apps/cli/src/tui/view_assembler/live_status.rs`
- `apps/cli/src/tui/view_model/live_status.rs`
- 对应 assembler/view-state/live-status tests。

**测试先行：**
- root live、primary absent 时 outer Spinner 仍存在且连续累积。
- primary absent 时 inner elapsed/phase text 不显示，不复用旧 phase。
- 新 primary 到达后 inner 从 Runtime sample 开始。
- heartbeat 只刷新对应权威 baseline，不改变 lifecycle identity。
- root terminal 后 Spinner 消失并清理两套 baseline。

**实现：**
- `ActivitySummary` 以 root 为必需、primary 为可选。
- `SpinnerLineView.phase_elapsed_secs` 改为 `Option<u64>`，与 `phase_text` 同步可选。
- `RunActivityState` 分开 `sync_root` / `sync_primary` / terminal clear 语义。

### Task 9：治理与退役

**修改：**
- `docs/design/02-modules/runtime/events/04-activity.md`
- `docs/design/02-modules/runtime/events/05-published-state.md`
- `docs/design/02-modules/tui/02-model.md`
- `docs/design/02-modules/tui/03-event-flow-and-acl.md`
- `docs/design/03-engineering/01-architecture-guards.md`
- `.agents/hooks/check-runtime-activity-observation.sh` 或对应 guard/probe。

**守卫：**
- 禁止 Runtime production `ActivityChanged`。
- 禁止 coordinator mutation 内直接 publication。
- 禁止 phase transition 发布中间 Snapshot。
- 禁止 TUI canonical mirror 依赖 ActivityChanged。
- 保留 SDK compatibility variant 与 first ACL 白名单。

### Task 10：完整验证与交付

**验证：**
- 各层 focused tests。
- `cargo test -p sdk`
- `cargo test -p runtime --lib`
- `cargo test -p cli --bin aemeath`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo run -p xtask -- sdk-wire-schema check`
- `.agents/hooks/check-architecture-guards.sh --full`
- `cargo fmt --all -- --check`
- `git diff --check`
- 实机 debug 日志验证一个多 tool/model phase Run：outer 连续、inner 重置、无 transient disappearance。

**交付：**
- 独立只读审查。
- commit、push。
- 中文更新 PR #1541、Issues #945/#1502。
- 检查远端 CI；保持 PR OPEN。

## 4. 风险与控制

1. **心跳 payload 增长**：必须限制 Snapshot graph；若暂不裁剪 terminal history，应在测试中锁定代表性容量并登记后续清理。
2. **手动 compact 独立 coordinator**：其 lifecycle 不一定经过 Main Run heartbeat task，必须决定并测试它是否复用 active Run coordinator，或拥有同生命周期 heartbeat。
3. **compatibility 双路径**：SDK `ActivityChanged` 保留不等于 TUI canonical production 双读；ACL 后必须归一到 Snapshot replacement 或明确 compatibility-only mutation。
4. **同 revision timing 更新**：TUI 必须用 heartbeat_sequence 接纳新 sample，不能因 business revision 相同丢弃。
5. **terminal Snapshot**：root terminal Snapshot 需要先到达 TUI，再停止 heartbeat；不能先 cancel heartbeat 而丢最终状态。
