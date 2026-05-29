# #59 S1 实现计划：spinner + task 入 Model 单源

> **For agentic workers:** 用 superpowers:subagent-driven-development 逐 Task 执行。每步独立编译 + `cargo test -p cli` 通过。

**Goal:** 把 OutputArea 自持的 spinner（业务态 active+phase）与 task 状态行迁入 RuntimeModel/view_state，渲染从 LiveStatusViewModel 单向派生，消灭 30 处直改 widget 旁路，加 guard。

**Spec:** `docs/superpowers/specs/2026-05-29-tui-s1-spinner-task-single-source.md`（务必先读，含真相边界划分：业务态[active/phase]入 Model、动画态[frame/verb/elapsed]归 view_state、task 存预格式化 lines）。

**验证门禁（每 Task）:** `cargo test -p cli`、`cargo clippy -p cli`、`bash .agents/hooks/check-architecture-guards.sh`。所有改动在 worktree `feature/59-s1-spinner-task`。

---

## Phase 1：Model 层（不接线）

### Task 1.1 SpinnerModel + SpinnerPhase + Intent/Change + reducer 测试
**Files:** Create `apps/cli/src/tui/model/runtime/spinner.rs`；Modify `model/runtime/{mod.rs,model.rs,intent.rs,change.rs,task_status.rs}`。
- `spinner.rs`: `pub struct SpinnerModel { pub active: bool, pub phase: Option<SpinnerPhase> }` + `pub enum SpinnerPhase { Thinking, Generating, AgentWorking, Reflecting, ThinkingQueued, CallingTool(String), CallingTools { remaining: usize }, Hook { event: String, detail: String, outcome: HookOutcome } }` + `pub enum HookOutcome { Running, Blocked, Timeout, Done }`。derive Clone,Debug,Eq,PartialEq（Hash 若 Model 需要）。Default：active=false,phase=None。
- `RuntimeModel` 增 `pub spinner: SpinnerModel`（Default）。
- `task_status.rs`：`TaskStatusSnapshot` 增 `pub lines: Vec<String>`（Default 空）。
- `intent.rs`：`RuntimeIntent` 增 `StartSpinner`、`SetSpinnerPhase(SpinnerPhase)`、`StopSpinner`、`UpdateTaskLines(Vec<String>)`。
- `change.rs`：`RuntimeChange` 增对应 `SpinnerStarted`、`SpinnerPhaseChanged`、`SpinnerStopped`、`TaskLinesChanged`。
- `model.rs::apply` 处理新 Intent：StartSpinner→active=true；SetSpinnerPhase→active=true+phase=Some(p)；StopSpinner→active=false,phase=None（幂等）；UpdateTaskLines→task_status.lines=lines。
- 测试（model.rs 末尾 mod tests）：start 置 active；set_phase 在 inactive 时自动 active 且 phase 正确；stop 幂等（重复 stop 不 panic、active 保持 false）；update_task_lines 写入。
- **Steps:** 写测试→验证失败→实现→验证通过→`cargo test -p cli spinner`→commit `feat(tui): SpinnerModel/SpinnerPhase + Intent/Change (refs #59 S1)`。

## Phase 2：view_state 动画 + 渲染派生（不接线）

### Task 2.1 view_state 动画态 + LiveStatusViewModel + assembler + adapter
**Files:** Modify `view_state/`（加 spinner 动画态 frame/verb）；Create `view_model/live_status.rs`、`view_assembler/live_status.rs`、`adapter/live_status_widget.rs`；注册 mod。
- view_state：新增 `SpinnerAnim { frame: u64, verb: String }`（或并入现有 animation state）。
- `view_model/live_status.rs`：`LiveStatusViewModel { spinner: Option<SpinnerLineView>, task_lines: Vec<String> }`；`SpinnerLineView { glyph_frame: u64, verb: String, elapsed_secs: u64, phase_text: Option<String> }`（或直接产 spans——与现 build_spinner_line 对齐，复用其渲染逻辑）。
- `view_assembler/live_status.rs`：`assemble(runtime: &RuntimeModel, anim: &SpinnerAnim) -> LiveStatusViewModel`：active 时据 frame/verb + phase→文案 产 SpinnerLineView；phase→文案转换函数 `fn phase_text(p: &SpinnerPhase) -> String`（集中文案：Thinking→"Thinking...", CallingTool(n)→format!("Calling {n}..."), Hook→format!("Hook {event} {outcome-suffix}") 等，对齐现有字面量）；task_lines 取 runtime.task_status.lines。
- `adapter/live_status_widget.rs`：`apply_live_status_to_widget(output_area, vm)` 唯一写回 `output_area.spinner`/`task_status_lines` 镜像。注意 widget 的 spinner 是 `Option<SpinnerState>`（含 Instant）——adapter 据 vm 构造/更新镜像（verb 随机选在此或 view_state 首次 active 时定）。
- 测试：每个 SpinnerPhase→phase_text 正确；frame→glyph；无 spinner→None；task_lines 透传。
- **Steps:** TDD→commit `feat(tui): LiveStatusViewModel + assembler + adapter (refs #59 S1)`。

## Phase 3：接线（行为不变）

### Task 3.1 SpinnerTick→view_state；update_task_status→Intent；adapter 写回
**Files:** Modify `app/update.rs`（SpinnerTick 推进 view_state frame；refresh 处调 apply_live_status_to_widget）、`app/runtime.rs`（update_task_status 改发 UpdateTaskLines Intent）、`run_loop.rs`。
- SpinnerTick：原 `tick_spinner()` → 推进 view_state.anim.frame。
- update_task_status：`agent_client.task_status().await` 拿 lines 后，经 `model.runtime.apply(UpdateTaskLines(lines))` 而非 `set_task_status`（拉取 async 保留，仅改写入路径）。
- 每帧/每次 refresh：assembler(runtime, anim) → apply_live_status_to_widget。
- 全量 `cargo test -p cli` 通过（行为不变）。commit `feat(tui): 接线 spinner/task 经 Model 派生 (refs #59 S1)`。

## Phase 4：触发点收敛

### Task 4.1 30 处 start/stop/set_phase → Intent
**Files:** `app/update/ui_event.rs`（~20 处）、`ask_user_key.rs`、`enter.rs`、`done.rs`、`app/slash/reflection.rs`、`run_loop.rs`。
- 逐处把 `self.output_area.start_spinner()` → 发 `RuntimeIntent::StartSpinner`；`set_spinner_phase("Thinking...")` → `SetSpinnerPhase(SpinnerPhase::Thinking)`；`set_spinner_phase(format!("Calling {name}..."))` → `SetSpinnerPhase(CallingTool(name))`；`stop_spinner()` → `StopSpinner`。Hook phase 映射到 `Hook{event,detail,outcome}`。
- Intent 经 reducer 应用；adapter 写回使 widget 镜像更新。
- 改 `status_line.rs`/`selection_tests.rs` 等直填 spinner/task 的测试为经 adapter 或直填镜像（不弱化断言）。
- 全量测试通过。commit `refactor(tui): spinner/task 触发点收敛为 Intent (refs #59 S1)`。

## Phase 5：guard

### Task 5.1 check-tui-spinner-task-single-source.sh
**Files:** Create `.agents/hooks/check-tui-spinner-task-single-source.sh`；Modify `check-architecture-guards.sh`。
- 仿 `check-tui-status-single-source.sh`：grep `apps/cli/src/tui` 排除 `live_status_widget.rs`/`spinner.rs`，FAIL 若出现 `\.start_spinner\(|\.stop_spinner\(|\.set_spinner_phase\(|\.set_task_status\(` 或 `output_area\.spinner\s*=`/`\.task_status_lines\s*=` 直写。可执行，clean 树退出 0，注入违规退出 1（验证）。
- 接入 orchestrator。全量测试 + clippy + 所有 hook 通过。commit `feat(tui): spinner/task 单源 guard (refs #59 S1)`。

## Self-Review
- 真相边界：业务态(active/phase)入 Model、动画态(frame/verb)归 view_state ✓
- 文案集中在 assembler phase_text ✓
- task 存预格式化 lines（不重建 items）✓
- 渲染 live tail 仍不入 block 缓存、每帧重组、动画不变 ✓
- guard 验证非空 ✓
