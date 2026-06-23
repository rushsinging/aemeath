# Runtime Context 职责边界梳理

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/456
>
> 状态：草案（待评审）

## 现状问题

### 三层 Context 字段重叠

`ChatRuntimeContext` → `TuiLaunchContext` → `ChatLoopContext` 构成一条手工字段拷贝链：

```
ChatRuntimeContext (port.rs)
  └─ accessors.rs: tui_launch_context()  ──手动拷 19 字段──▶  TuiLaunchContext (tui_launch.rs)
                                                                 └─ composition/app.rs 消费
                                                                 └─ trait_chat.rs: chat_impl()  ──手动拷 25 字段──▶  ChatLoopContext (loop_runner.rs)
```

每加一个 runtime 配置字段（如 `reasoning_graph`），要改 3-4 处 struct + 2 处构造代码 + N 处测试。当前重叠字段 15+ 个。

### 其他问题

| 问题 | 位置 | 影响 |
|---|---|---|
| `cwd` 与 `workspace` 双轨 | `ChatLoopContext`、`ToolExecutionContext` | `cwd` 已被 `workspace` 取代（loop_runner L128 从 workspace 读 root），但仍作为独立字段传递，造成"两个真相源" |
| SDK 边界类型不一致 | `TuiLaunchContext` 用 `sdk::*View`，`ChatRuntimeContext`/`ChatLoopContext` 用 `share::*` | 同一概念三种表示，转换靠手动 map |
| `user_context` 语义模糊 | 三层都有 `user_context: String` | 无文档说明承载什么，用途不清 |
| `verbose` 散落 | `ChatRuntimeContext`、`TuiLaunchContext` | `ChatLoopContext` 不含——说明它是启动期参数，不应在 context 链中传递 |

## 设计目标

1. **提取共享 base**——三层重叠的"不变共享件"提取为一个值类型，各 context 持有其引用/克隆。
2. **消除 `cwd` 双轨**——统一走 `workspace: Arc<WorkspaceService>`。
3. **明确每个 context 的语义边界**——加文档注释，明确生命周期和职责。
4. **SDK 边界统一**——`TuiLaunchContext` 的 SDK 视图转换集中在 composition root，不在 runtime 内部。

## 分类：哪些是真正的 Context，哪些不是

经过审视，13 个 `*Context*` 类型分为四类：

### A. 执行上下文（本次重构主体）

| 类型 | 生命周期 | 职责 |
|---|---|---|
| `ChatRuntimeContext` | session 级 | runtime↔上层端口契约，持有不变共享件 |
| `TuiLaunchContext` | 启动瞬间 | TUI 启动过渡 DTO（应瘦身为 SDK 视图） |
| `ChatLoopContext` | loop 级（消费式） | 单次 chat loop 全部状态 |
| `ToolExecutionContext` | 单次 tool call | tool 执行环境 |
| `CommandContext` | 单次命令 | slash 命令执行环境 |

### B. 标识 / DTO（职责清晰，不需要动）

| 类型 | 职责 |
|---|---|
| `RuntimeTurnContext` | 极简 ID 容器（chat_id + turn_id），事件路由用 |
| `PromptContext` | prompt 渲染参数（cwd + model 标识） |
| `PersistedWorkspaceContext` | session 落盘 DTO |
| `BackgroundTaskContext` | 后台任务状态 DTO |

### C. 统计 / 错误 / i18n（与 context 概念无关，仅名称碰巧含 "Context"）

| 类型 | 实际职责 |
|---|---|
| `ContextUsage` | token 预算统计快照（建议不改名，已成惯例） |
| `ErrorContext` / `ErrorWithContext` | 错误定位信息 |
| `GitContextLabels` | i18n 文案 |

**结论**：B 和 C 不动。本次重构聚焦 A 类的 5 个执行上下文。

## 重构方案

### Step 1: 提取 `RuntimeResources` 共享内核

从 `ChatRuntimeContext` 中提取"跨 session/loop/tool 不变的共享件"：

```rust
/// Runtime 不变共享件——跨 session/loop/tool 传递的同一组资源。
///
/// 所有 Arc 字段指向同一份底层实例，克隆开销极低。
/// session 级以下的所有 context（ChatLoopContext / ToolExecutionContext 等）
/// 持有此结构的 clone，不再各自重复声明这些字段。
#[derive(Clone)]
pub struct RuntimeResources {
    // ── 服务句柄（Arc 共享）──
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub task_store: Arc<TaskStore>,
    pub hook_runner: HookRunner,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,

    // ── 配置（值类型，session 期间不变）──
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub memory_config: MemoryConfig,
    pub skills_map: HashMap<String, Skill>,
    pub context_size: usize,
    pub allow_all: bool,
    pub language: String,

    // ── Reasoning Graph 配置（session 级，loop 时实例化）──
    pub reasoning_graph_config: Option<GraphRuntimeConfig>,
}
```

> **不含的字段**（因不属于"不变共享件"）：
> - `verbose: bool` — 启动期参数，移到 `AgentClientImpl` 字段，不在 context 链传递
> - `resume: Option<String>` — 启动期参数，构造完 loop 即丢弃
> - `reasoning_graph: Option<ReasoningGraph>` — loop 级实例（从 config 构造），不是共享件

### Step 2: `ChatRuntimeContext` 瘦身为端口入参

```rust
/// `ChatRuntimePort` 方法的入参——runtime 启动时的一次性配置包。
///
/// 持有 `RuntimeResources`（共享件）+ 启动期专有参数。
/// 构造完 `ChatLoopContext` 后不再存活。
#[derive(Clone)]
pub struct ChatRuntimeContext {
    pub resources: RuntimeResources,
    pub workspace: Arc<WorkspaceService>,
    pub verbose: bool,
    pub resume: Option<String>,
}
```

### Step 3: `TuiLaunchContext` 瘦身为 SDK 投影

`TuiLaunchContext` 的消费方（`composition/app.rs`）只用 `session_id`、`cwd`、`model_display`、`client.is_reasoning()`、`allow_all`、`context_size`、`memory_config`(SDK View)、`skills_map`(SDK View)。

将这些合并为一个 SDK 边界 DTO，**不再持有 runtime 内部类型**：

```rust
/// TUI 启动所需的 SDK 层投影（不含 runtime 内部类型）。
///
/// composition root 从 `AgentClientImpl` 构造此结构，
/// 传递给 CLI 入口。runtime 内部不再使用此类型。
pub struct TuiLaunchContext {
    pub session_id: String,
    pub model_display: String,
    pub allow_all: bool,
    pub context_size: usize,
    pub thinking: bool,
    pub memory_config: MemoryConfigView,
    pub skills_map: HashMap<String, SkillView>,
}
```

> `cwd` 从此结构移除——composition root 可直接从 `workspace` 读取。
> `session_reminders` 移除——它是 loop 级运行时状态，不应出现在启动投影中。

### Step 4: `ChatLoopContext` 持有 `RuntimeResources` + loop 专属状态

```rust
/// 单次 chat loop 的完整执行状态。
///
/// 由 `chat_impl()` 从 `AgentClientImpl.resources` 构造，
/// 按值传入 `process_chat_loop()`，函数内解构消费。
pub struct ChatLoopContext<S, Q, I> {
    // ── 不变共享件 ──
    pub resources: RuntimeResources,

    // ── loop 端口 ──
    pub sink: S,
    pub queue: Q,
    pub input_events: I,

    // ── loop 专属可变状态 ──
    pub messages: Vec<Message>,
    pub workspace: Arc<WorkspaceService>,
    pub session_id: String,
    pub read_files: Arc<Mutex<HashSet<String>>>,
    pub session_reminders: Arc<Mutex<SessionReminders>>,
    pub cancel: Arc<Mutex<CancellationToken>>,
    pub frozen_chats: Arc<Mutex<Vec<ChatSegment>>>,
    pub active_summary: Arc<Mutex<Option<String>>>,
    pub reasoning_graph: Option<ReasoningGraph>,
}
```

> 变化：`cwd` 移除（从 `workspace` 读取）。`max_tool_concurrency`/`max_agent_concurrency` 移到 `AgentClientImpl`，通过参数传入而非 context 字段。

### Step 5: `ToolExecutionContext` 持有 `ToolResources`（tools crate 内定义）

> **跨 crate 约束**：`ToolExecutionContext` 在 `tools` crate，`RuntimeResources` 在 `runtime` crate。
> 依赖方向 `runtime → tools`，`tools` 不能反向依赖 `runtime`，因此不能直接嵌入 `RuntimeResources`。
> 此外 `RuntimeResources` 的字段类型（`LlmClient`、`HookRunner`、`SystemBlock`、`Skill` 等）来自
> `provider`/`hook`/`prompt` crate，`tools` 不依赖这些 crate。

方案：在 **tools crate** 定义 `ToolResources`，只含 tool 执行实际需要的字段。
runtime 构造 `ToolExecutionContext` 时从 `RuntimeResources` 映射相关字段。

```rust
// tools/src/contract/resources.rs
/// Tool 执行所需的共享资源（tools crate 自包含）。
///
/// 由 runtime 的 `RuntimeResources` 构造时映射填充。
#[derive(Clone)]
pub struct ToolResources {
    pub agent_runner: Option<Arc<dyn AgentRunner>>,
    pub registry: Option<Arc<dyn ToolListProvider>>,
    pub memory_config: MemoryConfig,
    pub lang: String,
    pub allow_all: bool,
}

// tools/src/contract/context.rs
#[derive(Clone)]
pub struct ToolExecutionContext {
    pub resources: ToolResources,

    // ── tool 执行专属（每次构造不同）──
    pub workspace: Arc<WorkspaceService>,
    pub cancel: CancellationToken,
    pub read_files: Arc<Mutex<HashSet<String>>>,
    pub session_reminders: Option<Arc<Mutex<SessionReminders>>>,
    pub plan_mode: Option<bool>,
    pub progress_tx: Option<mpsc::Sender<AgentProgressEvent>>,
    pub parent_session_id: Option<String>,

    // ── runtime 专属（通过 ctx 携带供 agent runner 读取）──
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
}
```

> 变化：
> - `cwd` **移除**——改从 `workspace_read().current_workspace_root()` 读取
> - `agent_runner`/`registry`/`memory_config`/`lang`/`allow_all` 合并到 `ToolResources`
> - `max_tool_concurrency`/`max_agent_concurrency`/`agent_semaphore` 保留在顶层（runtime 的 agent runner 读，不属于"不变共享件"）

### 字段迁移总表

| 原字段 | 迁移目标 | 说明 |
|---|---|---|
| client, registry, task_store, hook_runner, agent_runner, agent_semaphore | `RuntimeResources` | Arc 共享件 |
| system_blocks, system_prompt_text, user_context, memory_config, skills_map, context_size, allow_all, language | `RuntimeResources` | 值类型配置 |
| reasoning_graph_config | `RuntimeResources` | session 级配置（loop 实例化） |
| verbose | `AgentClientImpl` 字段 | 启动期参数，不进 context 链 |
| resume | `ChatRuntimeContext` 保留 | 启动后丢弃 |
| workspace | 各 context 各自持有 | 非"不变"——loop 内可变 |
| messages | `ChatLoopContext` 保留 | loop 专属可变状态 |
| cancel | `ChatLoopContext` / `ToolExecutionContext` 保留 | 生命周期不同 |
| session_id | 各 context 各自持有 | 标识用途 |
| read_files, session_reminders, frozen_chats, active_summary | `ChatLoopContext` 保留 | loop 专属可变状态 |
| reasoning_graph (instance) | `ChatLoopContext` 保留 | loop 级实例 |
| cwd | **移除** | 从 `workspace` 读取 |
| max_tool/agent_concurrency | `AgentClientImpl` 字段 | 启动期参数 |

## 执行计划

按"先加新的、再迁移消费者、最后删旧的"策略，每步可独立编译通过。

### Phase 1: 创建 `RuntimeResources`（非破坏性）

1. 新建 `agent/features/runtime/src/core/resources.rs`，定义 `RuntimeResources`
2. `ChatRuntimeContext` 增加 `pub resources: RuntimeResources` 字段（旧字段暂时保留）
3. 构造 `ChatRuntimeContext` 处同步填 `resources`
4. `cargo test` 通过（零行为变更）

### Phase 2: 迁移 `ChatLoopContext`

1. `ChatLoopContext` 增加 `pub resources: RuntimeResources` 字段
2. `chat_impl()` 构造时从 `inner.context.resources` 填充
3. `process_chat_loop()` 内从 `resources` 读字段（旧字段暂时保留）
4. loop_runner_tests 逐个迁移到 `resources` 字段
5. 移除 `ChatLoopContext` 中已进 `resources` 的旧字段
6. `cargo test` 通过

### Phase 3: 迁移 `ToolExecutionContext`

> **跨 crate 约束**：`ToolExecutionContext` 在 `tools` crate，`RuntimeResources` 在 `runtime` crate。
> 依赖方向不允许 `tools → runtime`。方案：在 `tools` crate 定义 `ToolResources`。

1. 在 `tools/src/contract/resources.rs` 定义 `ToolResources`（5 字段：`agent_runner`、`registry`、`memory_config`、`lang`、`allow_all`）
2. `ToolExecutionContext` 增加 `pub resources: ToolResources` 字段
3. loop_runner 构造处从 `RuntimeResources` 映射字段填充 `ToolResources`
4. 各 tool 实现从 `ctx.resources.xxx` 读字段
5. 移除 `cwd` 字段（tools crate 内 3 处已改从 `workspace_read().current_workspace_root()` 读）
6. 移除已进 `resources` 的 5 个旧字段
7. `cargo test` 通过

### Phase 4: 瘦身 `TuiLaunchContext` + `ChatRuntimeContext`

1. `TuiLaunchContext` 改为纯 SDK 投影（移除 runtime 内部类型）
2. `composition/app.rs` 适配
3. `ChatRuntimeContext` 移除已进 `resources` 的旧字段
4. `cwd` 从所有 context 移除
5. `cargo test` 通过

### Phase 5: 文档 + 架构守卫

1. 每个 context 类型加文档注释（职责、生命周期）
2. 更新 `runtime-design.md` 增加 "Context 类型" 章节
3. 评估是否加架构守卫（如禁止 `cwd` 字段出现在 context 类型中）

## 验证

- 每个 Phase 结束 `cargo test --workspace` 全绿
- 最终 `cargo clippy --workspace -- -D warnings` 全绿
- 架构守卫通过
- 实机测试：`echo "你好" | AEMEATH_VERSION= RUST_LOG= cargo run -- -qv` 正常回复

## 风险

| 风险 | 缓解 |
|---|---|
| `ChatLoopContext` 在测试中被大量手构（10+ 处），迁移工作量较大 | Phase 2 步骤 4 专门处理测试迁移 |
| `ToolExecutionContext` 被 10+ 个 tool 实现引用，改动面大 | Phase 3 分批迁移，先加新字段，逐个 tool 切换 |
| `TuiLaunchContext` 是 SDK 边界，改动影响 composition root | Phase 4 最后做，此时其他 context 已稳定 |
| `cwd` 移除可能影响 hook 环境变量注入 | loop_runner L128 已从 workspace 读 root，验证 hook env 仍正确 |
