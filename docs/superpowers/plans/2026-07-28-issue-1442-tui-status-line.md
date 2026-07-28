# Issue #1442 TUI status line 根因级修复实施计划

> **对应 Issue：** https://github.com/rushsinging/aemeath/issues/1442
> **Milestone：** `v0.1.0 — Context Engineering + 架构重构`
> **执行方式：** 在 `fix/1442-tui-status-line` worktree 中按任务顺序执行 TDD；先写失败复现测试，再修改生产代码。跨层配置链路必须逐层保留相邻契约测试，并用 TUI 场景证明最终组合。

## 目标

修复 TUI 底部 status line 的三项问题，同时收敛状态与配置的数据所有权：

- 第一行 busy / 非 idle graph phase 使用明确的 Running 语义和 `TOOL_RUNNING` 主题色；Ready、普通状态和警告保持原有语义。
- 第一行、第二行 status line 均取消 `│` 分隔，统一使用空格分隔；窄屏缩减、宽度预算、选择和复制仍保持确定性。
- policy 展示完整反映 `ask`、`auto_read`、`allow_all`，启动时和 ConfigChanged / ConfigReloaded 后都与 committed `ConfigView` 一致。
- `StatusBar` 不再保存 permission mode 的默认值或可变镜像；TUI 继续只消费 SDK Config DTO，不读取 ConfigReader、配置文件或环境变量。

## 现状与根因

### 现状证据

- `apps/cli/src/tui/render/status/bar.rs` 的 `RuntimeSegmentStyle::Status` 只有 `Normal / Success / Warning`；`StatusNoticeViewKind` 同样没有 Running。graph phase 通过 `notice_from_phase` / `SetGraphPhase` 映射为 `StatusNotice::normal`，所以忙状态落入普通文本色。
- `bar.rs::runtime_segments` 直接追加 `"│"`；`status_bar_format.rs` 以 `FIELD_SEPARATOR = " │ "` 参与第二行字段拼接、宽度预算和紧凑 fallback。
- `StatusLineContext::default()` 将 permission mode 固定为 `"AskMe"`，`StatusBar` 暴露 `set_permission_mode` 维护 widget 内部镜像。
- `apps/cli/src/chat.rs` 启动时仅根据 `bootstrap.allow_all: bool` 写入 `AllowAll` 或 `AskMe`，无法表达 `AutoRead`。
- `TuiConfigView.permission_mode` 和 `sdk::ConfigView.permission_mode` 已包含三态字符串；但 `App::update_runtime_event` 对 ConfigChanged / ConfigReloaded 只更新 Markdown spacing，未更新完整 `App.config_view`。
- status view assembler 只从 Conversation / Runtime / Workspace / Session / Diagnostic 组装，没有把 App 的 ConfigView 投影进 StatusViewModel，因此配置 DTO 不是 status line 的唯一展示来源。

### 根因结论

这是三个展示层结构缺口，而不是三个独立的字符串修补问题：

1. **状态语义缺失：** busy 被压缩到 Normal，renderer 没有可靠的 Running 分支。
2. **布局规则分散：** 两行各自硬编码 separator，导致视觉、宽度和选择边界容易漂移。
3. **配置所有权错误：** 三态 policy 在 Runtime/SDK 已存在，却在 Composition → CLI → StatusBar 边界被压缩成 bool，并在 StatusBar 内再次生成默认值；reload 事件也没有完整更新 TUI 的 ConfigView。

## 核心设计决策

### 1. Running 是独立的状态语义

扩展 `StatusNoticeKind` / `StatusNoticeViewKind` 增加 `Running`，由非 idle graph phase 产生。`StatusViewAssembler` 只负责保留该语义，`StatusBar` 将 Running 映射至 `theme::TOOL_RUNNING`。不根据展示文本是否包含 `busy`、`thinking` 或其他字符串猜测颜色。

`StatusNotice::normal` 继续表示普通文本状态；`Ready` 的现有 Success 语义和 Warning 语义不改。需要区分“运行中”和“普通通知”的生产状态路径必须在 model 层明确产生 Running。

### 2. 两行使用同一空格分隔契约

在 status formatter 中定义唯一的空格字段 separator，例如 `"  "`，runtime segments 复用同一个布局常量或等价的 owner-local formatter。separator 的显示宽度必须同时用于：

- runtime/context 字段拼接；
- 窄屏和 fallback 的可用宽度计算；
- context 行彩色 span 拆分；
- status selection 的字符索引与复制文本。

生产输出不得包含 `│`。不改变字段顺序、截断优先级或状态栏背景。

### 3. ConfigView 是 policy 展示的单一真相

保留三层边界：

```text
ConfigSnapshot.permission_mode()
  → Runtime::config_snapshot_to_sdk
  → sdk::ConfigView.permission_mode
  → TUI adapter::TuiConfigView
  → App.config_view
  → StatusViewModel.permission_mode
  → StatusBar renderer
```

TUI-owned `StatusPermissionMode` 可以是显式 enum，也可以在 adapter/model 边界使用受控字符串；禁止让 `StatusBar` 自己把未知值静默变成 AskMe。未知/空值只允许通过明确的兼容 fallback 处理，并要有测试证明；正常三态必须一一映射为 `Ask`、`AutoRead`、`AllowAll`。

启动路径使用 `bootstrap.config_view`，不再从 `bootstrap.allow_all` 推导 status 展示。Runtime 内部 `allow_all` 仍可作为 Run scope 执行快照，但不得替代三态展示 DTO。

ConfigChanged / ConfigReloaded 处理时完整更新 `App.config_view`；spacing 等其他配置更新继续走现有专责 intent。状态栏的 permission mode 从 `App.config_view` 投影，而不是由 `StatusBar::set_permission_mode` 写入。

### 4. 删除 StatusBar 配置镜像

删除 `StatusLineContext.permission_mode` 默认值、`StatusBar::set_permission_mode` 及其 CLI 调用；将 permission mode 加入 `StatusContextViewModel` 或 `StatusViewModel`，由上层一次性组装。`StatusBar` 保持无状态渲染 widget，只消费 view model 和 selection。

## 文件结构与范围

### 生产代码

- 修改 `apps/cli/src/tui/model/conversation/status_notice.rs`
  - 增加 Running 状态构造和相关语义测试。
- 修改 `apps/cli/src/tui/model/conversation/runtime_state.rs`
  - 非 idle graph phase 投影为 Running，idle 继续 Ready。
- 修改 `apps/cli/src/tui/view_model/status.rs`
  - 增加 TUI status permission mode 字段及 Running view kind。
- 修改 `apps/cli/src/tui/view_assembler/status.rs`
  - 投影 Running、permission mode；禁止从文本猜测状态。
- 修改 `apps/cli/src/tui/render/status/bar.rs`
  - 渲染 Running 颜色；移除 permission setter；使用统一空格 separator；保持 selection 行为。
- 修改 `apps/cli/src/tui/render/display/status_bar_format.rs`
  - 将第二行 separator 改为空格并统一宽度/fallback 计算；删除默认 `AskMe` 镜像。
- 修改 `apps/cli/src/chat.rs`
  - 启动时写入 `bootstrap.config_view`，删除从 `allow_all` 到 status bar 的 bool 映射。
- 修改 `apps/cli/src/tui/app/update.rs`
  - ConfigChanged / ConfigReloaded 完整更新 `App.config_view`，保留已有 spacing 投影和 dirty 语义。
- 按实际调用链修改 `apps/cli/src/tui/adapter/agent_event.rs`、`event_mapping.rs`、`tui_runtime_event.rs` 或对应 view/adapter 文件；不得将 SDK 类型越过 ACL 直接带入 view model。

### 测试代码

- 修改/新增 `apps/cli/src/tui/model/conversation/status_notice_tests.rs` 或当前模块测试文件。
- 修改 `apps/cli/src/tui/render/display/status_bar_tests.rs`、`status_bar_v2_tests.rs`：Running 色、无竖线、空格宽度、三态 policy 和 selection。
- 修改 `apps/cli/src/tui/adapter/status_widget.rs`：model → view 的 Running/policy 投影。
- 修改 `apps/cli/src/tui/adapter/event_mapping_tests.rs`：ConfigView.permission_mode 三态 ACL 字段完整性。
- 修改 `apps/cli/src/tui/app/scenario_tests/startup.rs` 或新增 status line 场景：三态启动展示、busy 展示。
- 修改/新增 `apps/cli/src/tui/app/scenario_tests/config.rs`（若当前模块无合适文件）：ConfigChanged / ConfigReloaded 后 policy 立即更新。
- 测试组织遵守 `specs/3.2-rust-coding.md` 与 `docs/design/03-engineering/04-testing-and-coverage.md`；大型测试外置，不批量迁移无关历史测试。

## 实施任务

### Task 1：锁定状态语义与 renderer 失败证据

- [ ] 新增失败测试：非 idle graph phase 组装为 Running；idle 仍为 Ready；普通 notice 仍为 Normal；Warning 不变。
- [ ] 新增失败渲染测试：Running status 首行 cell 使用 `theme::TOOL_RUNNING`，而非 `TEXT`。
- [ ] 新增失败测试：第一、第二行文本均不包含 `│`，并包含预期字段间空格。
- [ ] 新增失败测试：空格 separator 参与宽度预算和窄屏 fallback，不造成超宽输出或字段删除优先级改变。
- [ ] 运行定向测试，记录失败原因，确认失败来自缺少语义/布局实现而非测试 fixture。

### Task 2：实现 Running 状态语义

- [ ] 在 model/view model 中增加 Running variant 和构造映射。
- [ ] 修改 `notice_from_phase`，非 idle phase 返回 Running；不要通过字符串匹配 phase 名称。
- [ ] 更新 `StatusViewAssembler` 和 `StatusBar::runtime_segment_style`。
- [ ] 重跑 Task 1 的 model、adapter、renderer 测试，确认 Running 色和其他状态色无回归。

### Task 3：统一空格分隔布局

- [ ] 定义并复用唯一空格 separator；移除 runtime/context 的 `│` 硬编码。
- [ ] 同步 context formatter 的 fields join、separator_count、shrink/fallback 宽度计算。
- [ ] 同步彩色 span 拆分逻辑，使 separator 本身使用布局色或基础文本色且不恢复竖线。
- [ ] 检查 status selection 的屏幕列到字符索引、复制文本和窄屏截断，不依赖旧 separator 长度。
- [ ] 重跑所有 status bar formatter/render/selection 定向测试。

### Task 4：把 ConfigView 三态接入 StatusViewModel

- [ ] 先新增三态映射失败测试：`ask`、`auto_read`、`allow_all` 分别得到 `Ask`、`AutoRead`、`AllowAll`；空/未知值按明确兼容策略处理。
- [ ] 将 permission mode 加入 StatusViewModel 的受控字段；在 status assembler 入口传入 TUI 当前 ConfigView，而非读取 ConfigReader。
- [ ] 修改 `App::status_view_model` 和相关 view assembler 接口，确保 status renderer 只消费 ViewModel。
- [ ] 删除 `StatusLineContext` 默认 `AskMe`、`set_permission_mode`、`set_permission_mode_for_test` 及 CLI 启动 setter。
- [ ] 启动路径将 `bootstrap.config_view` 写入 `app.config_view`，policy 显示不再依赖 `bootstrap.allow_all`。
- [ ] 重跑 adapter/model/renderer 定向测试，确认三态显示和 StatusBar 无可变配置镜像。

### Task 5：贯通 ConfigChanged / ConfigReloaded reload 链路

- [ ] 先新增失败场景：启动为 Ask，发送带 `allow_all` 的 ConfigReloaded 后，status line 更新为 AllowAll；再发送 AutoRead，更新为 AutoRead。
- [ ] 在 `App::update_runtime_event` 完整替换或更新 `self.config_view`，同时保留 spacing 映射和原有 system notice。
- [ ] 确认 event mapping 保留 `permission_mode`，不在 SDK → TUI adapter 过程丢字段。
- [ ] 确认 config event 触发 status dirty/re-render；不重建 TUI、logger、MCP 或 storage。
- [ ] 运行 ConfigView ACL、TUI adapter 和 L4 场景测试。

### Task 6：清理旧路径与全量验证

- [ ] 搜索并确认不存在旧旁路：

```bash
rg -n 'set_permission_mode|permission_mode: "AskMe"|" │ "|"│"|bootstrap\.allow_all' apps/cli/src
```

允许的命中必须仅是语义测试 fixture、文档历史或 Runtime 执行语义；StatusBar 生产代码不得保留旧镜像/分隔符。

- [ ] 运行：

```bash
cargo fmt --all -- --check
git diff --check
cargo build -p cli
cargo test -p cli
cargo clippy -p cli --all-targets --all-features -- -D warnings
bash .agents/hooks/check-architecture-guards.sh
```

- [ ] 记录测试层级：L1 状态/布局纯逻辑；L2 model→view→renderer 协作；L3 SDK→TUI DTO ACL；L4 启动/reload/busy TUI 场景；L5 真实终端 smoke 如环境可用，否则说明 N/A。
- [ ] 检查无废弃 setter、默认 fallback、只被测试引用的生产 API；更新 Issue #1442 checklist 与验证证据，但不关闭 Issue。

## 验收门禁映射

| Issue 验收项 | 证据 |
|---|---|
| busy 有颜色 | Running 状态 L1 + view/renderer L2 + TUI 场景 L4，断言 `TOOL_RUNNING` |
| 分隔符改为空格 | formatter/runtime renderer/selection L1-L2 + framebuffer L4，断言无 `│` |
| policy 跟随 config | SDK→TUI ACL L3 + 启动/reload L4，覆盖 Ask/AutoRead/AllowAll |
| StatusBar 无配置镜像 | 生产代码搜索、架构守卫、ViewModel 单一来源测试 |
| 构建与质量门禁 | `cargo build -p cli`、`cargo test -p cli`、`cargo clippy -p cli`、架构守卫 |

## 风险与回滚边界

- **状态颜色回归：** Running 只新增一个明确 variant，不改变 Success/Normal/Warning 的既有色彩；若某些 phase 需要不同颜色，先增加语义映射测试，不以字符串猜测补丁。
- **布局宽度回归：** separator 长度改变会影响窄屏算法和 selection；必须同步更新 display width 预算，禁止只替换文本常量。
- **配置三态丢失：** `allow_all` 仍为 Runtime Run snapshot 的执行字段，不能删除；它只是不能作为 TUI policy 展示来源。
- **reload stale state：** 完整更新 `App.config_view` 并使 status view 每帧从该值派生，禁止新增第二份 permission mirror。
- **兼容 fallback：** 旧 SDK/事件缺少 permission mode 时使用已有 DTO 默认值，并对该行为单独测试；正常三态不允许静默折叠。
- **范围控制：** 不改 policy 执行授权规则、不修改 Config 优先级、不引入新环境变量、不重建 TUI 基础设施、不修改 session 持久化格式。

## 完成后的 PR 门禁

- [ ] `git diff --check`、定向测试、CLI 构建、CLI clippy、架构守卫全部有结果。
- [ ] 按仓库流程在分支上执行 `git pull origin main` 后再推送。
- [ ] 创建指向 `main` 的非 Draft PR，关联 `Closes #1442`；未经用户对具体 PR 当前 head 明确授权，不合并 PR、不关闭 Issue。
