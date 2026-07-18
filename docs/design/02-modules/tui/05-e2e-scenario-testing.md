# TUI · E2E 场景测试

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：[#1006](https://github.com/rushsinging/aemeath/issues/1006)
> 本文定义基于 ratatui `TestBackend`、crossterm 事件类型与 insta 快照的进程内 TUI 端到端场景测试流程。

## 1. 定位与边界

TUI 场景测试验证一条完整的进程内用户可见链路：

```text
crossterm Event
  → TuiMsg
  → update / reducer
  → Model + ViewState
  → Effect
  → Effect 结果回灌
  → ViewAssembler / ViewModel
  → ratatui TestBackend framebuffer
  → insta snapshot
```

它的目标不是替代现有单元测试，而是捕获跨层组合错误，例如事件已到达但未触发 dirty、Effect 未回灌、ViewAssembler 丢字段、Render 绕过消费入口或布局在特定尺寸下错位。全仓 L0-L5 测试分层、目录组织、覆盖率与生产可达性规则见 [04-testing-and-coverage.md](../../03-engineering/04-testing-and-coverage.md)。

### 1.1 测试类型边界

| 类型 | 覆盖 | 不覆盖 |
|---|---|---|
| 纯逻辑单元测试 | Intent、reducer、状态转换、格式化 | 完整屏幕组合 |
| Buffer/widget 测试 | 单个 widget、cell 样式、选区与 gutter | 上游事件和 Effect |
| **进程内场景测试** | 事件到 framebuffer 的完整 TUI 链路 | 真实 TTY、转义序列、raw mode |
| PTY smoke test | 进程启动、alternate screen、raw mode、退出恢复 | 细粒度状态组合 |

`TestBackend` 不模拟真实终端协议。因此 raw mode、alternate screen、`EventStream`、信号和 panic 后终端恢复属于少量 PTY smoke test，不进入本文的快照基线。

### 1.2 核心原则

1. **MUST** 使用生产的 update、Effect 协议、ViewAssembler 与 Render 路径，不复制业务状态转换。
2. **MUST** 使用真实 crossterm 事件类型表达按键、鼠标、粘贴和 resize。
3. **MUST** 由测试 Effect Driver 隔离网络、剪贴板、文件系统、计时器和后台任务。
4. **MUST** 在稳定检查点渲染，不依赖 `sleep` 等待异步系统“碰巧稳定”。
5. **MUST** 同时保留语义断言与屏幕快照；快照不能替代 Effect payload 和 Model 不变量断言。
6. **NEVER** 在测试中调用全局 `crossterm::event::read`、真实 `EventStream` 或真实系统剪贴板。
7. **NEVER** 通过修改全局 cwd、共享环境变量或系统时钟构造 fixture。

## 2. 方案选择

### 2.1 最小方案

将 `App::draw` 从固定的 `Terminal<CrosstermBackend<Stdout>>` 泛化为接受任意 ratatui backend。测试依次调用 `App::update`、dirty flush 和 `draw(TestBackend)`。

优点：

- 改动小，能快速建立第一批屏幕快照；
- 复用现有 App、ViewAssembler 和 Render。

风险：

- Harness 需要复制生产 `run_loop` 的“更新 → Effect → 回灌 → flush → draw”顺序；
- 生产循环变化后，测试驱动流程可能静默漂移；
- 难以覆盖异步 Effect 和多事件回灌的真实边界。

### 2.2 根因方案（目标态）

从生产 `run_loop` 提取一个与终端来源无关的**单帧驱动器**。生产环境只负责把 `EventStream`、timer、signal 和 Runtime stream 转成 `TuiMsg`，单帧驱动器统一负责：

```text
接收 TuiMsg
  → App::update
  → 产出 Effect
  → EffectDriver 执行或排队
  → 接收回灌 TuiMsg
  → flush dirty ViewModel
  → 同步滚动和动画 ViewState
  → render
```

生产使用真实 Effect Driver，场景测试使用确定性的 Test Effect Driver。两者共享同一帧语义，避免 Harness 复制 `run_loop`。

优点：

- 生产与测试共享事件处理顺序；
- Effect 边界可观测、可脚本化；
- 后续新增 timer、Runtime 事件或 render 请求时不需要维护第二套循环。

成本与风险：

- 需要先整理 `run_loop`、`draw` 和 Effect executor 的边界；
- 若抽取时把终端 IO 或 tokio task 泄漏进单帧驱动器，会形成新的不可测耦合。

**决策**：实现时优先根因方案；最小方案仅允许作为第一阶段脚手架，并必须在同一实施子树中记录退役步骤。

## 3. 测试架构

### 3.1 组件图

```text
Scenario
  │ steps: key / mouse / paste / resize / runtime event / tick
  ▼
TuiScenarioHarness
  ├─ App / TuiModel / AppViewState
  ├─ FrameDriver（生产共享）
  ├─ TestEffectDriver
  ├─ VirtualClock
  ├─ VecDeque<TuiMsg>（确定性回灌队列）
  └─ Terminal<TestBackend>
          │
          ▼
     normalized screen
          │
          ├─ semantic assertions
          └─ insta snapshot
```

### 3.2 `TuiScenarioHarness`

Harness 是场景测试唯一入口，至少提供以下能力：

| 能力 | 语义 |
|---|---|
| `new(size, fixture)` | 用固定 session、workspace、provider/model 和终端尺寸启动 |
| `key(key_event)` | 注入 crossterm `KeyEvent` |
| `paste(text)` | 注入 paste 消息，不访问系统剪贴板 |
| `mouse(mouse_event)` | 注入 crossterm `MouseEvent` |
| `resize(width, height)` | 调整 TestBackend 并注入 resize |
| `runtime(event)` | 注入 Runtime/Agent 经 ACL 前的公开事件 fixture |
| `tick()` | 虚拟时钟前进一个离散 tick |
| `drain()` | 按 FIFO 处理全部已排队回灌消息 |
| `run_until(predicate, max_steps)` | 有上限地推进到稳定条件 |
| `render()` | 执行生产共享的 flush、同步和 draw |
| `snapshot(name)` | 对规范化屏幕执行 insta 断言 |
| `assert_idle()` | 断言无待处理消息、Effect、timer 和后台任务 |

场景 API 应表达用户动作和外部事实，**NEVER** 直接修改 Model 私有字段。少量初始 fixture 必须经公开构造器、Intent 或已定义的测试 builder 建立。

### 3.3 crossterm 输入构造器

测试使用 `KeyEvent`、`MouseEvent`、`MouseEventKind`、`KeyCode`、`KeyModifiers` 和 `KeyEventKind::Press`，确保组合键语义与生产一致。可提供无状态辅助构造器：

- 普通字符和字符串输入；
- Enter、Esc、Tab、Backspace、方向键、PageUp/PageDown、Home/End；
- Ctrl+C、Ctrl+V 等组合键；
- 鼠标 Down/Drag/Up/Scroll；
- paste 与 resize。

字符输入默认拆成多个 `KeyEvent`，只有 paste 场景使用完整字符串，避免两类输入路径被错误合并。

## 4. 单帧驱动与 Effect

### 4.1 帧语义

每个动作按以下固定顺序执行：

1. 将外部动作转换为一个 `TuiMsg`；
2. 调用生产 update；
3. 收集 `UpdateResult` 中的 Effect；
4. Test Effect Driver 按声明顺序执行 Effect；
5. Effect 产生的结果以新 `TuiMsg` 进入 FIFO 队列；
6. 场景显式选择处理一个消息、drain 全部消息或停在中间态；
7. flush dirty ViewModel；
8. 同步 live status、scroll、selection 和动画 ViewState；
9. 向 `TestBackend` 绘制一帧；
10. 执行语义断言和可选快照。

场景可以在工具 Running、AskUser 等中间态停住并快照，但不能在队列状态不明确时隐式等待。

### 4.2 Test Effect Driver

| Effect 类别 | 测试行为 |
|---|---|
| `RequestRender` | 记录请求，由帧驱动统一 render，禁止递归 draw |
| `QuitApplication` | 走生产退出状态变更 |
| `SendChatInputEvent` | 记录精确 payload，并按脚本产生 Runtime 事件 |
| cancel/reset | 调用 fake port，回灌预设 accepted/completed/error 事件 |
| timer | 注册/移除虚拟 timer，由 `tick()` 触发 |
| clipboard | 记录复制文本，读取操作返回预设成功或失败 |
| image/file | 返回内存 fixture，不读真实路径 |
| update/memory/reflection | 由脚本返回预设事件，不调用真实服务 |
| spawn/stream | 注册受 Harness 管理的逻辑任务，不启动不可追踪 task |

每个脚本步骤必须声明预期 Effect 类型和关键 payload。收到未声明 Effect 时立即失败；场景结束存在未消费脚本时同样失败。

### 4.3 Runtime/Agent 脚本

Runtime fixture 使用有序事件脚本表达一轮交互，例如：

```text
expect UserMessage("读取 Cargo.toml")
  → UserMessagesAdopted
  → TurnStarted
  → Thinking("我先检查文件")
  → ToolCallStart(Read)
  → ToolCallRunning(Read)
  → ToolResult(...)
  → AssistantText("完成")
  → TurnCompleted
```

事件 ID、顺序和 payload 均由 fixture 固定。脚本必须允许场景逐步推进，以便分别检查 Thinking、Tool Running 和完成态。

## 5. TestBackend 与屏幕规范化

### 5.1 固定尺寸矩阵

| 名称 | 尺寸 | 用途 |
|---|---:|---|
| standard | `100×30` | 默认交互和完整屏幕基线 |
| narrow | `40×20` | wrapping、建议列表和状态栏降级 |
| tiny | `19×7` | 小于最小绘制阈值时不 panic |
| wide | `140×40` | 长工具结果、diff 和宽状态栏 |

除专门验证 resize 的场景外，一个快照只能对应一个明确尺寸，尺寸必须进入快照名。

### 5.2 屏幕输出

主快照保存完整 framebuffer 的可见文本，保留：

- 空白行和布局边界；
- Unicode marker、gutter、scrollbar 和宽字符；
- output、input、suggestions、status、dialog 的相对位置。

快照前只允许规范化终端尾部空白和 ratatui 明确的 continuation cell；**NEVER** 过滤业务文本、顺序或布局差异。

### 5.3 样式验证

文本快照不能完整表达颜色和 modifier。颜色、背景、选区与宽字符 continuation cell 继续由 Buffer cell 单元测试显式断言。若需要跨组件样式快照，应额外生成稳定的语义 cell 表示：

```text
(row, col, symbol, semantic_fg, semantic_bg, modifiers)
```

该表示使用语义颜色名而非 RGB 值，避免主题微调导致所有场景快照失效。主屏幕快照不混入样式元数据。

## 6. insta 快照治理

### 6.1 稳定检查点

快照只放在用户可感知且事件队列状态明确的检查点：

- 初始首帧；
- 输入完成但尚未提交；
- 提交后等待 Runtime；
- Thinking 或 Tool Running 中间态；
- Tool Result 完成态；
- AskUser 选中/确认态；
- 最终回答或错误态；
- resize、滚动或 dialog 操作后的稳定帧。

streaming 每个 chunk 不建快照。流式顺序由语义断言覆盖，屏幕只保留代表性中间态和完成态。

### 6.2 命名

采用 `{scenario}__{checkpoint}__{width}x{height}`，例如：

- `startup__ready__100x30`
- `chat_submit__thinking__100x30`
- `chat_submit__completed__100x30`
- `tool_read__running__100x30`
- `ask_user__second_option__100x30`
- `resize__wrapped__40x20`

快照文件与场景模块相邻保存在 `snapshots/`，避免全仓单一目录。

### 6.3 审阅流程

开发者流程：

1. 定向运行场景测试，生成 `.snap.new`；
2. 使用 `cargo insta review` 逐项审阅；
3. 结合语义断言确认变化来自预期行为，而非 fixture 漂移；
4. 接受的 `.snap` 与对应代码变更在同一 PR 提交；
5. 拒绝并删除无意变化。

CI 使用 `CI=1` 与 `INSTA_UPDATE=no`，并检查不存在 `.snap.new` 和 `.pending-snap`。CI **NEVER** 自动接受或重写快照。#1017 当前将该行为落为本地/离线 `scripts/check-tui-snapshots.sh`；是否进入在线 PR CI 由 #1018 按耗时决策。

## 7. 确定性约束

以下输入必须固定或可注入：

| 非确定来源 | 处理方式 |
|---|---|
| session/chat/turn/tool/input ID | fixture 固定值或确定性 ID generator |
| cwd/workspace/home/git branch | `WorkspaceFixture` 注入，禁止探测宿主机 |
| provider/model/config | 固定 `ConfigView` fixture |
| 当前时间、notice TTL | `VirtualClock` |
| spinner frame/verb | 显式 tick 与固定随机源 |
| Runtime stream 时序 | FIFO 脚本逐步推进 |
| update check、网络、剪贴板、图片 | Test Effect Driver fixture |
| 终端尺寸 | TestBackend 固定尺寸 |
| Unicode 宽度 | 固定依赖版本和 fixture 字符集 |

`run_until` 必须带 `max_steps`。超限错误至少输出：当前 checkpoint、最后一个消息、待处理消息、待执行 Effect、虚拟时间、Model 核心状态和当前屏幕，禁止无界等待。

## 8. 场景矩阵

### 8.1 P0 核心路径

| 场景 | 输入/事件 | 检查点 | 关键语义断言 |
|---|---|---|---|
| 启动首帧 | 创建 Harness | ready | banner、输入框、status、workspace 可见 |
| 输入并提交 | 字符键 + Enter | typed / submitted | document 内容正确；发送单一 UserMessage Effect |
| 流式回答 | adopted → turn → chunks → complete | thinking / completed | chunk 顺序、dirty、完成后 spinner 停止 |
| 工具生命周期 | start → ready → running → result → complete | running / completed | tool 绑定、marker、result gutter、最终状态 |
| AskUserQuestion | questions + 上下键 + Enter | shown / selected / confirmed | phase、选项索引、回答 payload、dialog 消失 |
| slash suggestions | `/` + 导航 + 选择 | suggestions / executed | completion 来源、选中项、Effect 或 pending slash |
| 取消与退出 | Ctrl+C、再次 Ctrl+C 或 `/quit` | cancelling / exit | cancel 只发一次；退出状态和提示正确 |

### 8.2 P1 布局与交互

| 场景 | 重点 |
|---|---|
| resize | 重排、wrap、scroll max、status 降级 |
| 长输出滚动 | follow-tail、PageUp/PageDown、Home/End |
| 鼠标选择与复制 | hit test、gutter 跳过、Copy Effect payload |
| paste | 文本、多行、空 paste、图片路径分类 |
| narrow/tiny | 不 panic、不越界、不产生非法 Rect |
| 错误与重试 | warning/error 显示、spinner 收敛、可重试提示 |
| task/compact | task line、compact progress 与 output dirty |
| worktree 切换 | status path、branch、工具路径及 cache 失效 |
| Main/Sub 投影 | 父 ToolCall 关联、嵌套展示与 role/model 元数据 |

### 8.3 场景退出不变量

每个场景结束必须检查：

1. 无未处理 TuiMsg；
2. 无未执行或未声明 Effect；
3. 无未消费 Runtime 脚本；
4. 无活跃虚拟 timer 或逻辑 task，除非场景显式声明；
5. App 未处于不可能的组合状态；
6. 最后一帧已在最新 Model revision 上完成组装和渲染。

## 9. 目录与依赖

目标目录按测试能力组织，而不是复制生产技术分层：

```text
apps/cli/src/tui/app/testing.rs
apps/cli/src/tui/app/testing/
  harness.rs
  effect_driver.rs
  fixture.rs
  input.rs

apps/cli/src/tui/app/scenario_tests.rs
apps/cli/src/tui/app/scenario_tests/
  startup.rs
  input.rs
  chat.rs
  tool.rs
  ask_user.rs
  layout.rs
  snapshots/
```

`testing.rs + testing/` 和 `scenario_tests.rs + scenario_tests/` 使用 Rust 2018+ 同名文件/目录并存，**NEVER** 使用 `mod.rs`；二者仅在 `cfg(test)` 下编译。`insta` 在 #1017 需要快照时再作为 `apps/cli` 的 dev-dependency 引入；ratatui 与 crossterm 复用生产版本。

当前 `cli` 只有 binary target，第一阶段使用 crate 内 `cfg(test)` 访问内部边界。只有当场景需要跨 crate 复用时，才将可复用 TUI 主体抽成 library target；不为追求 integration test 目录形式提前扩大公开 API。

## 10. CI 与验收

### 10.1 分层门禁

建议顺序：

1. reducer/model 单元测试；
2. ViewAssembler 与 Buffer/widget 测试；
3. P0 场景测试；
4. P1 场景测试；
5. CLI 全量测试与 clippy；
6. 独立的少量 PTY smoke test。

P0/P1、快照草稿检查和 PTY smoke 均先提供本地/离线入口并记录冷/热耗时。#1050 最终落地 `scripts/check-slow-test-matrix.sh`：host-native fmt/clippy/workspace/P0/P1/PTY 为必跑，跨 target build 仅在显式 `AEMEATH_MATRIX_CROSS=1` 且 toolchain/linker 可用时运行；不新增普通 PR workflow。

### 10.2 设计验收

实现完成至少满足：

- 生产与测试共享单帧处理顺序；
- crossterm 输入到最终 framebuffer 的核心链路有场景覆盖；
- Runtime/SDK/TUI ACL/Model/ViewAssembler/Render 关键层均有相邻测试或场景检查点；
- P0 场景在固定尺寸和固定 fixture 下可重复通过；
- 本地/离线快照检查禁止自动更新并拒绝遗留草稿；执行位置由 #1018 决定；
- 真实终端职责由单独 PTY smoke test 覆盖，边界不混淆。

## 11. 相关文档

- [01-architecture-and-dataflow.md](01-architecture-and-dataflow.md)：TEA 总体管线
- [03-event-flow-and-acl.md](03-event-flow-and-acl.md)：Runtime/SDK 事件进入 TUI 的 ACL
- [04-view-layer.md](04-view-layer.md)：ViewAssembler、ViewModel、ViewState 与 Render
- [../../03-engineering/01-architecture-guards.md](../../03-engineering/01-architecture-guards.md)：架构守卫与验证治理
- [../../03-engineering/04-testing-and-coverage.md](../../03-engineering/04-testing-and-coverage.md)：全仓测试分层、覆盖率、生产可达性与 dead-code 治理

## 12. 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-14 | 初稿：定义 TestBackend/crossterm/insta 场景测试架构、流程、矩阵与 CI 门禁 | [#1006](https://github.com/rushsinging/aemeath/issues/1006) |
| 2026-07-14 | 链接全仓测试架构、覆盖率与生产可达性治理 | [#677](https://github.com/rushsinging/aemeath/issues/677) |
| 2026-07-15 | 落地共享同步 Frame Driver、任意 Backend 基础 Harness 和 startup/input 证明场景；修正无 mod.rs 与耗时先行规则 | [#1016](https://github.com/rushsinging/aemeath/issues/1016) |
| 2026-07-15 | 扩展 Scripted Effect/Runtime 注入/离散 tick，落地 P0 快照、本地草稿检查和 completion busy Enter 根因修复 | [#1017](https://github.com/rushsinging/aemeath/issues/1017)、[#1009](https://github.com/rushsinging/aemeath/issues/1009) |
| 2026-07-16 | 落地代表性 P1 组合场景、真实 PTY 启动/恢复 smoke 与 host/cross 慢速矩阵入口 | [#1050](https://github.com/rushsinging/aemeath/issues/1050) |
