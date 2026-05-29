# #59 S5-gap：剩余豁免项 Effect 化（A 块）+ slash 主分发 wontfix（B 块）

**日期**：2026-05-29
**所属**：feature #59 子项 **S5** 剩余 gap（reflection/clipboard 已 Effect 化）
**前置**：S5 部分完成；既有 Effect 基础设施（effect/effect.rs + executor.rs，结果经 UiEvent 回灌）；`cached_sessions` 预取范本

## 裁定（调研结论，诚实范围）
`check-tui-tea-purity.sh` EXEMPT 剩余 9 项分两类：

### A 块（真违反、做）
- **dialog.rs / suggestions.rs** 的 `block_in_place + block_on(list_models)`：list_models 是配置派生、会话期内基本不变的静态数据，无需实时拉取。**suggestions.rs 在 update 纯路径内**（每次按键可能 block_on），最严重。
- **save.rs** 的 `.await`（sync_current_messages + save_current_session）：已有完全对应的 `Effect::SaveSession`。
- **memory.rs** 的 `.await`（list_reminders）：近似 `Effect::FetchReminderRecap`，加镜像 Effect。
- **slash.rs:142 /paste** 的 `block_on(read_clipboard_image)`：已有 `Effect::ReadClipboardImage`。

### B 块（真违反但 wontfix）
- **slash.rs 主分发管线**（~13 处 request-response `.await`：/compact、/context、execute_command 分发、handle_command_action 的 switch_model/set_thinking/load_session/reset）：是 **request-response + 返回 `Option<String>` 控制流**语义——命令需 IO 返回值做即时同步 UI 反馈与 prompt 注入决策。Effect 化需把每命令拆成"发 Effect + UiEvent 回流续接"状态机，引入大量 pending 状态、破坏 `Some(prompt)` 直返、重写 slash_tests。**收益仅 guard 名单少一项，成本高 → wontfix**。
- **mod.rs**（git Command 同步元数据探测）、**run_loop.rs / runtime.rs**（runtime 编排层本身的 `.await`，TEA 副作用执行器所在）、**slash_tests.rs**（测试 mock）：合理边界，**wontfix**。

## A 块实现

### A1. model 列表预取/缓存（消除 dialog/suggestions block_on）— 最高价值
照搬 `cached_sessions` 范本：
- `app/state/session.rs`：`SessionState` 加 `cached_models: Vec<sdk::ModelSummary>`（或 `(provider,name)`，按 dialog/suggestions 用法）+ `cache_models(...)` + 读取器。
- `app/runtime.rs`：加 `refresh_model_cache()`（async，调 `agent_client.list_models()` 写缓存），紧邻 `refresh_session_cache`。
- `app/session/session_lifecycle.rs`：启动期 + session 切换处调用（紧邻 refresh_session_cache）。
- `slash/dialog.rs::open_model_selection_dialog` + `slash/suggestions.rs::update_suggestions`：改读 `self.session.cached_models`，去掉 block_in_place/block_on → **变纯函数**，移出 EXEMPT。
- 无需新 Effect。

### A2. /save → 复用 `Effect::SaveSession`
`slash.rs:94` /save 改 push `Effect::SaveSession`（executor 已实现 sync+save）。保留 `[session saved: id]` / `Failed` 反馈：executor save 路径加成功/失败 UiEvent（或复用既有 `EffectResult::SessionSaved`，effect.rs:41）回流，update 据此推送反馈行。`handle_save_command` 删除/转纯，save.rs 移出 EXEMPT。

### A3. /memory → 新增 `Effect::FetchMemoryList`
镜像 `FetchReminderRecap`：effect.rs 加 `FetchMemoryList`，executor list_reminders 后发 `UiEvent::MemoryList`。`slash/memory.rs` 改 push Effect → 转纯，移出 EXEMPT。

### A4. /paste → 复用 `Effect::ReadClipboardImage`
`slash.rs:142` block_on 改 push 既有 `Effect::ReadClipboardImage`（executor 已实现）。该行去 block_on。

### A5. guard 收紧
`check-tui-tea-purity.sh`：把 dialog.rs/suggestions.rs/save.rs/memory.rs 移出整文件 EXEMPT（已转纯/Effect）。slash.rs 改为**行级豁免**（`// allow tea_side_effect` 注释豁免主分发 await 行，B 块 wontfix），不再整文件豁免——除非行级豁免机制不存在，则保留 slash.rs 整文件豁免但在 roadmap/spec 标注 B 块 wontfix 原因。mod.rs/run_loop/runtime/slash_tests 保留豁免（wontfix）。
roadmap（`docs/feature/active.md` #59 S5）更新：标注 A 块完成、B 块 wontfix 理由。

## 非目标 / wontfix
- slash.rs 主分发管线 Effect 化（B 块，request-response 语义，过度工程）。
- mod.rs git Command / run_loop / runtime / slash_tests（runtime/测试边界）。

## 迁移分步（每步独立编译 + `cargo test -p cli` 通过）
1. **A1**：`cached_models` + `refresh_model_cache` + 启动/切换接线；dialog/suggestions 转纯读缓存；移出 EXEMPT。（最高价值，先做）
2. **A2**：/save 复用 SaveSession + 反馈回流；save.rs 移出。
3. **A3**：`Effect::FetchMemoryList` + executor + memory.rs 转纯移出。
4. **A4**：/paste 复用 ReadClipboardImage。
5. **A5**：guard 收紧（移出 A 块文件、slash.rs 行级豁免或标注）+ roadmap 标 A 完成/B wontfix；全量 test+clippy+hook。

## 测试
- A1：cached_models 预取/读取；dialog/suggestions 纯函数读缓存产出正确（model 列表为空/多项）。
- A2：/save 推 SaveSession Effect + 反馈 UiEvent。
- A3：FetchMemoryList Effect + executor 发 MemoryList。
- A4：/paste 推 ReadClipboardImage。
- 行为不变：/model 对话框、补全候选、/save 反馈、/memory 列表、/paste 图片。
