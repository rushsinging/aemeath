# Issue #1411：TUI 完整历史、compact 解耦与 3000 行窗口计划

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1411
> 状态：待实施
> 目标分支：`fix/1411-tui-history-window`

## 目标

修复 resume 后 compact 前历史无法在 TUI 中继续加载的问题，并保留现有的历史窗口交互：

1. compact 只影响 LLM active context，不影响用户可见历史；
2. resume 后 TUI 可浏览 session 中 compact 前后的完整 RunStep 历史；
3. 初始最多渲染 1000 行，滚到顶部每次最多增加 500 行；
4. 历史渲染窗口设置 3000 行上限；
5. 加载以完整 `RenderedBlock` 为边界，不截断 block；
6. 用户上滚后，LLM 流式输出不改变当前视口；
7. 回到底部恢复跟随最新输出并收口到默认 1000 行；
8. 保留自动加载诊断日志，并避免“无更多历史”状态重复刷 debug。

## 已确认根因

当前 `SessionRestore::from_canonical()` 通过 `flattened_steps_from_marker()` 同时生成 Runtime 恢复消息和 TUI 展示步骤。该方法从 compact marker 开始过滤历史，因此 TUI 收到的不是完整 session 历史。

当前 TUI 的分批加载逻辑本身已通过真实日志确认正常：真实 session 的可见 source 从 1000 行扩展到 1500 行，再扩展到 1843 行；之后 `expanded=false` 是因为上游 projection 已经没有更早内容，而不是顶部触发失败。

## 设计边界

### Active context 与 display history 双投影

Context 必须继续维护单一 `CanonicalSession` backing，但从同一 backing 生成两个只读投影：

- **Active projection**：从 compact marker 开始，供 Runtime 恢复 LLM backing 和后续上下文构建；包含 compact summary 语义，不暴露给 TUI。
- **Display projection**：遍历全部 `run_slices`，包含 compact marker 前后的所有 RunStep，供 `SessionResumed` 事件传递给 SDK/TUI。

两者共享同一套消息完整性清理逻辑，不复制 sanitize/deep-clean 实现。展示投影只处理内存副本，不因 resume 或渲染修改持久化 session。

### 3000 行窗口上限

窗口状态采用以下预算：

- `INITIAL_RENDER_LINES = 1000`；
- `HISTORY_LOAD_BATCH_LINES = 500`；
- `MAX_RENDER_LINES = 3000`。

顶部加载时：

```text
next_limit = min(current_limit + 500, source_total_lines, 3000)
```

达到 3000 行后，即使 source 仍有更早历史，也不能继续扩大当前渲染文档。实现必须保留当前视口锚点，并按完整 `RenderedBlock` 选取窗口内容。

本阶段不实现完整双向虚拟列表，不在向上加载过程中淘汰当前窗口底部内容。回到底部时：

```text
scroll_offset = 0
auto_scroll = true
render_line_limit = 1000
pending_load_older = false
```

之后重新渲染最近的 1000 行。若未来需要在历史位置持续浏览超过 3000 行，再单独设计带游标的双向虚拟窗口。

### 日志

保留以下诊断日志：

- `debug`：历史窗口成功扩展、首次兑现 pending 请求、首次进入“已无更多历史”；
- `trace`：每次顶部请求、`expanded=false` 的重复请求、每帧 source/render 指标。

日志不得输出完整消息正文。`expanded=false` 的高频重复状态不得继续使用 `debug`。

## 分层实施任务

### Phase 1：Context 双投影与失败测试

**目标**：先证明 compact 不得影响 display history。

1. 在 Context restore/domain 测试中新增 compact session fixture：marker 前至少两个 Step，marker 后至少两个 Step。
2. 新增失败断言：active projection 只包含 marker 后 Step；display projection 包含全部 Step，并保留 `run_id` / `step_id`。
3. 新增无 compact 场景：active/display 投影内容一致。
4. 新增消息完整性测试：display projection 清理孤儿消息时不修改 canonical backing。
5. 将 `SessionResumeProjection` 字段改为明确的 `active_messages` 与 `display_steps`，避免继续复用含义模糊的 `messages` / `steps`。
6. 实现从全部 `run_slices` 生成 display steps 的 Context-owned 方法，并复用单一清理函数。

**涉及路径**：

- `agent/features/context/src/domain/session/restore.rs`
- `agent/features/context/src/domain/session/restore_tests.rs`
- `agent/features/context/src/domain/session/envelope.rs`
- `agent/features/context/src/domain/session/management.rs`
- `agent/features/context/src/adapters/session_resume.rs`

**验证**：

```bash
cargo test -p context restore
cargo test -p context session_management
```

### Phase 2：Runtime resume 语义隔离

**目标**：Runtime 使用 active context，事件发布完整 display history。

1. 修改启动 resume 与运行期 `/resume` 共用的 `resume_session_to_backing` 消费新双投影。
2. 仅将 `active_messages` 写入 Runtime backing / session continuation。
3. 将 `display_steps` 映射为 `RuntimeStreamEvent::SessionResumed`。
4. 增加 Runtime 集成测试，验证 compact 前 Step 不进入 active messages，但进入 SessionResumed event。
5. 增加两条 resume 路径等价测试，防止 startup resume 和 slash resume 分叉。

**涉及路径**：

- `agent/features/runtime/src/application/client/resume_helper.rs`
- `agent/features/runtime/src/application/main_loop/looping/loop_runner.rs`
- `agent/features/runtime/tests/main_session_wiring_integration.rs`
- `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`

**验证**：

```bash
cargo test -p runtime resume
cargo test -p runtime main_session_wiring_integration
```

### Phase 3：SDK / Adapter 契约保持完整字段

**目标**：确保完整 display steps 穿过 SDK 和 TUI adapter，不被中间层过滤或覆盖。

1. 保持 `ResumedSessionStep { run_id, step_id, messages }` 作为展示 DTO；必要时补充注释明确其为完整 display history。
2. 增加 Runtime event → SDK `ChatEvent::SessionResumed` 的 compact 前后 Step 透传测试。
3. 增加 SDK → TUI adapter 的字段保真测试，验证顺序、RunStep identity 和消息数量。
4. 检查 startup 与 slash resume 的事件映射均走相同 DTO。

**涉及路径**：

- `packages/sdk/src/chat_event.rs`
- `packages/sdk/tests/session_resume_contract.rs`
- `agent/features/runtime/src/adapters/event_projection.rs`
- `agent/features/runtime/src/adapters/event_projection_tests.rs`
- `apps/cli/src/tui/adapter/event_mapping.rs`
- `apps/cli/src/tui/adapter/event_mapping_tests.rs`

**验证**：

```bash
cargo test -p sdk session_resume
cargo test -p runtime event_projection
cargo test -p cli event_mapping
```

### Phase 4：TUI 3000 行窗口与视口行为

**目标**：完整 display history 在 TUI 中按 500 行批次加载，并在 3000 行收口。

1. 将窗口常量集中定义为 1000 / 500 / 3000，禁止在多个文件重复定义。
2. 顶部加载使用 `min(current + 500, source_total, 3000)`。
3. 达到 3000 行时不再扩大渲染窗口，并保持当前可见内容稳定。
4. 保留完整 `RenderedBlock` 边界；预算允许因 block 完整性略微超过目标，但不得跨越 3000 行上限选择更多 block。
5. 回到底部重置到 1000 行并恢复 auto-scroll。
6. 用户上滚期间追加 LLM 输出，按现有 offset 补偿保持视口固定；不得因窗口收口把当前可视 block 删除。
7. 自动加载日志按既定 debug/trace 等级记录。

**涉及路径**：

- `apps/cli/src/tui/view_state/output.rs`
- `apps/cli/src/tui/view_state/output_tests.rs`
- `apps/cli/src/tui/app/update.rs`
- `apps/cli/src/tui/app/update/key_scroll.rs`
- `apps/cli/src/tui/render/output/rendered.rs`
- `apps/cli/src/tui/app/scenario_tests/history_window.rs`

**验证**：

```bash
cargo test -p cli history_window -- --nocapture
cargo test -p cli output
```

场景测试必须覆盖：

- 普通长历史连续加载；
- compact 前后完整历史连续加载；
- 1000 → 1500 → 2000 → 2500 → 3000 的预算变化；
- 3000 行后继续到顶不再扩大；
- block 不截断；
- 上滚期间流式输出视口不变；
- 回到底部后恢复最新输出和 1000 行窗口。

### Phase 5：集成验证与旧路径检查

1. 用真实 session `019f9952-601d-7139-a936-fa5d1f366eb9` resume，确认 TUI source 包含 compact 前历史。
2. 观察 `tui.log`：应能看到扩展到 3000 的过程；达到上限后只保留 trace 级重复请求，不刷 debug。
3. 验证下一次 LLM 请求仍只使用 active context，不把 compact 前展示历史重新发送给模型。
4. 检查旧的 `flattened_steps_from_marker()` 消费点，确保只用于 active context，不再被 display resume 使用。
5. 删除无消费者的临时兼容字段或旧 helper，避免保留第二套历史投影逻辑。
6. 执行格式、编译、定向测试和相关架构守卫。

**验证命令**：

```bash
cargo fmt --all -- --check
cargo test -p context
cargo test -p runtime
cargo test -p sdk
cargo test -p cli
cargo check --workspace
```

## 不在本计划范围

- 不改变 session schema 或已有落盘文件格式；
- 不新增真正的文件级随机分页读取；
- 不建立第二个可变 session backing；
- 不实现超过 3000 行的双向虚拟列表；
- 不展示伪造的 compact system message；如需显示 compact 分界线，另行定义 typed display boundary；
- 不修改 compact 算法、summary 生成或 token 预算规则。

## 完成定义

- Context active/display 双投影测试通过；
- Runtime、SDK、TUI 每层字段保真测试通过；
- compact 前历史可在 TUI 中浏览；
- 每次顶部加载最多增加 500 行，渲染窗口不超过 3000 行；
- 回到底部恢复 1000 行和自动跟随；
- 上滚期间 LLM 输出不移动视口；
- 日志保留且高频无更多历史状态为 trace；
- 全部相关 Cargo 测试和 `cargo check --workspace` 通过。
