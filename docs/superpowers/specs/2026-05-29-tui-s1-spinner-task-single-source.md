# #59 S1：spinner + task 状态行迁入 Model 单源

**日期**：2026-05-29
**所属**：feature #59（TUI Model/View 单源迁移收口）子项 **S1**
**前置**：roadmap `2026-05-29-tui-single-source-completion-roadmap.md`（§4 S1）；#58/#63 渲染管线已落地（live tail 不入 block 缓存，#63 未动 spinner/task 旁路）

## 问题
`OutputArea` widget 自持 spinner 动画态（`spinner: Option<SpinnerState>`）与 task 状态行（`task_status_lines: Vec<String>`），由 ~30 处散落在 `app/update/*`、`app/slash/reflection.rs`、`run_loop.rs` 的 `start_spinner`/`set_spinner_phase`/`stop_spinner`/`tick_spinner` 与 `set_task_status` 命令式直改。违反单源：UI 真相未在 Model，业务逻辑直接改 widget。现有 `check-tui-effect-boundary.sh` 只扫顶层 `update/`、不含 `app/update/`，故无 guard 拦截。

## 设计

### 真相边界划分（关键决策）
- **业务真相入 Model**：spinner **是否活跃** + **当前 phase**（语义），task **显示行快照**。
- **纯动画细节归 view_state**：spinner 的 `frame`/`verb`/`elapsed`（每 90ms SpinnerTick 推进的渲染易变态，非业务真相）。TEA 中 `view_state` 正是承载此类易变 UI 态的层。
- 理由：`SpinnerState.start: Instant` 无法进 Model（派生 `Clone/Eq/PartialEq`）；且 frame/verb 是装饰，把它们留在 view_state 既保留动画、又让 Model 纯净，同时消灭"业务逻辑直改 widget"的旁路（S1 的核心目标）。

### Model 层（`model/runtime/`）
- 新增 `SpinnerModel { active: bool, phase: Option<SpinnerPhase> }`（纯，无 Instant）。
- `SpinnerPhase` 语义枚举，归拢 30 处散落文案：
  `Thinking | Generating | AgentWorking | Reflecting | ThinkingQueued | CallingTool(String) | CallingTools { remaining: usize } | Hook { event: String, detail: String, outcome: HookOutcome }`（HookOutcome: Running/Blocked/Timeout/Done）。**文案（"Thinking..." 等）在 assembler 由 phase 转换**（DRY，集中一处），Model/Intent 只携带语义。
- `RuntimeModel` 增 `spinner: SpinnerModel`。
- task：扩展 `TaskStatusSnapshot` 增 `lines: Vec<String>`（预格式化显示行，存 agent 拉来的快照），或新增独立字段；**不重建逐条 items**——task 行真相在 agent 侧（`sdk::TaskStatusView.lines`），CLI 经 Model 中转即满足单源。
- 新增 `RuntimeIntent::{ StartSpinner, SetSpinnerPhase(SpinnerPhase), StopSpinner, UpdateTaskLines(Vec<String>) }` + 镜像 `RuntimeChange`。`apply` 三路径单测（含 StopSpinner 幂等、SetSpinnerPhase 在 inactive 时自动 active）。

### view_state 层（动画）
- spinner 动画态（frame/verb）移入 `view_state`（`AppViewState.animation` 或新 `SpinnerAnim { frame: u64, verb: String }`）。
- SpinnerTick → 推进 `frame`；`verb` 在 spinner 由 inactive→active 的那一刻选定一次（随机选择是 effectful，放在 view_state 更新处/adapter，**不在纯 reducer**）。`elapsed` 由 `frame * 90ms` 推算，无需 Instant。

### 渲染派生（`view_assembler/` + `view_model/` + `adapter/`）
- 新增 `view_model/live_status.rs`：`LiveStatusViewModel { spinner: Option<SpinnerLineView>, task_lines: Vec<String> }`。
- 新增 `view_assembler/live_status.rs`：从 `model.runtime.spinner`(active+phase) + `view_state` 动画(frame/verb) 派生 spinner 行 view（phase→文案在此转换）；task_lines 取自 Model。
- 新增 `adapter/live_status_widget.rs::apply_live_status_to_widget`：**唯一写回** widget 的 `spinner`/`task_status_lines` 镜像字段。
- `render/output_area/render.rs` + `output/status_line.rs::append_status_lines` **不变**（仍读 widget 镜像、live tail 仍不入 block 缓存、每帧重组，动画照常）。widget 字段降级为只读镜像。

### 触发点收敛
- 30 处 `start/stop/set_spinner_phase` → 改发对应 `RuntimeIntent`（经 `root_reducer` → `model.runtime.apply`）。
- `app/runtime.rs::update_task_status`（每帧 `run_loop.rs:33` 调，`agent_client.task_status().await` → 直 `set_task_status`）→ 拉到的 `view.lines` 改经 `RuntimeIntent::UpdateTaskLines` 存 Model（拉取本身的 async 若已有 `Effect::FetchTaskStatus` 则复用其结果回灌；否则保留拉取、仅改写入 Model——拉取 Effect 化属 S5 范围，S1 不强制）。
- `run_loop.rs` spinner 启动（:88）与 SpinnerTick：tick 推进 view_state.frame；启动经 Intent。
- adapter 在 `app/update.rs` 的 `refresh_output_widget_from_model` 并列处调用 `apply_live_status_to_widget`。

### Guard
新增 `.agents/hooks/check-tui-spinner-task-single-source.sh`（仿 `check-tui-status-single-source.sh`）：禁止 `live_status_widget.rs` 与 `spinner.rs` 之外出现 `output_area.spinner`/`.task_status_lines` 直写、或调 `start_spinner/stop_spinner/set_spinner_phase/set_task_status`。接入 `check-architecture-guards.sh`。

## 非目标
- 不重建 task 逐条 items（存预格式化 lines）。
- 不 Effect 化 task_status 拉取（属 S5）。
- 不动 #63 的 block 渲染树/gutter/选区。
- 不改 spinner 视觉（glyph/微光/verb 池/90ms 周期）。

## 迁移分步（每步独立编译 + `cargo test -p cli` 通过）
1. `SpinnerModel` + `SpinnerPhase` + `RuntimeIntent`/`RuntimeChange`（Start/SetPhase/Stop/UpdateTaskLines）+ reducer 三路径单测。**不接线**。
2. view_state 动画态（frame/verb）+ `LiveStatusViewModel` + `view_assembler/live_status.rs`（phase→文案）+ `adapter/live_status_widget.rs`，单测覆盖 phase 文案、frame→glyph、task lines 透传。
3. 接线：SpinnerTick→推进 view_state frame；`update_task_status`→`UpdateTaskLines`；adapter 在 update_ui 后写回；render 改读镜像（行为不变，全量测试通过）。
4. 30 处 `start/stop/set_phase` 逐文件替换为 Intent；改 `status_line.rs`/`selection_tests.rs` 等直填 spinner/task 的测试为经 adapter 或直填镜像。
5. 新增 guard + 接入；全量 `cargo test -p cli` + clippy + 所有 hook 通过。

## 测试
- reducer：Start/SetPhase（inactive→active）/Stop（幂等）/UpdateTaskLines 三路径。
- assembler：每个 SpinnerPhase → 正确文案；frame→glyph 微光；task_lines 透传；无 spinner 时 None。
- adapter：apply 写回 widget 镜像正确。
- 回归：现有 spinner/task 渲染测试（status_line.rs/selection_tests.rs）经新路径仍通过；live tail 不入 block 缓存不变。
