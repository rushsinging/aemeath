# Runtime 设计

## 定位

Runtime 是核心域的**唯一应用服务**，所有入站适配器（TUI / CLI / Server）接入同一组 `AgentClient` API。它不关心请求来自哪里——本地终端还是远端 WebSocket——只负责把一次用户输入推进成完整的 Agent 协作过程。

## 端口与适配器

```
        Inbound Ports                    Outbound Ports
    ┌──────────────────┐            ┌──────────────────┐
    │  AgentClient     │            │  ProviderPort    │
    │  (packages/sdk)  │            │  (LLM gateway)   │
    ├──────────────────┤            ├──────────────────┤
    │  Chat / Cancel   │            │  ToolPort        │
    │  Session Mgmt    │            │  (Tool trait)    │
    │  Subscribe       │            ├──────────────────┤
    └────────┬─────────┘            │  StoragePort     │
             │                      │  (持久化投影)     │
    ┌────────▼─────────┐            ├──────────────────┤
    │  Runtime App     │            │  PromptPort      │
    │  Service         │            │  (guidance)      │
    │  (agent/features/│            ├──────────────────┤
    │   runtime)       │            │  PolicyPort      │
    └──────────────────┘            │  (权限判断)       │
                                    ├──────────────────┤
                                    │  WorkspacePort   │
                                    │  (project)       │
                                    └──────────────────┘
```

## Agent Looping

核心域的状态机，驱动 Chat 从用户输入到最终响应的完整生命周期：

```
Session
  └── Chat
      └── Agent Looping
          ├── Main Turn
          ├── Child Turn（SubAgent）
          ├── ModelInvocation（调用 Provider 端口）
          ├── ToolExecution（调用 Tool 端口）
          ├── Task updates
          ├── AskUser pause / resume（通过入站端口回调）
          ├── Stop Hook（调用 Hook 端口）
          └── Final response
```

每个 Turn 通过端口与外部交互：
- **ProviderPort**：发送 ModelRequest，接收 streaming ChatEvent。
- **ToolPort**：收集 tool_use → 并发执行 → 结果注入回消息。
- **PromptPort**：加载 guidance / system prompt。
- **PolicyPort**：评估权限和风险。
- **StoragePort**：持久化 Session / Chat / Turn / Task 投影。

## Tool 执行编排

执行流程：LLM 返回 tool_use → Agent 收集 → 并发执行 → 结果注入回消息。

`Tool` trait 与 `ToolRegistry` 定义在 `agent/features/tools`；Runtime 只负责循环里的调度与结果回填。`ToolIdentityRegistry` 负责将 provider stream 信息映射到内部 id：

- `by_stream_index: HashMap<usize, ToolCallId>`
- `by_provider_id: HashMap<String, ToolCallId>`
- 新 id 由 `ToolCallId::new_v7()` 生成
- 同一 provider id **MUST** 复用同一内部 id
- provider id 缺失时，按 stream index 生成/复用；后续 provider id 出现时补齐映射

## Token Budget / 压缩 / 成本

- **Token 估算**：`agent/features/runtime/src/business/compact/token_estimation.rs`
- **成本追踪与定价**：`agent/features/runtime/src/business/cost/pricing.rs`
- **成本历史落盘**：`~/.agents/cost_history.json`
- 修改涉及暂停/恢复/重试逻辑时 **SHOULD** 同步更新 `token_estimation`
- 成本追踪逻辑更新时 **SHOULD** 同步更新 `pricing.rs`

## Slash 命令系统

通过 `inventory` crate + 注册表自动收集：

- 值类型 `CommandDescriptor`：`core/command.rs`
- 注册表：`core/command/registry.rs`（启动时遍历所有 `inventory::submit!` 的描述符）
- 命令模块：`core/command/commands/`（每个命令一个文件）

新增命令只需两步：
1. 在 `core/command/commands/` 下创建文件，用 `inventory::submit!` 声明
2. 在 `core/command/commands.rs` 注册该子模块

命令自动出现在 TUI 自动补全中，无需改 TUI 代码。

## 内部 ID 体系（UUIDv7）

内部实体 ID 与 provider 协议 ID 严格分离——核心域不依赖外部协议的 ID 约定：

| 层 | ID 类型 | 来源 | 用途 |
|---|---|---|---|
| 领域 | `ChatId` / `ChatTurnId` / `ToolCallId`（UUIDv7 newtype） | 核心域生成 | 跨 chat / turn / tool join |
| 协议 | `provider_id: String` | Provider 适配器返回 | 回填给 LLM 时使用 |
| 持久化 | 内部 UUIDv7 serde 为字符串 | Storage 适配器落盘 | 旧非 UUIDv7 id 加载时临时重新生成 |

### ToolCall 双 ID 结构

```rust
struct ToolCall {
    id: ToolCallId,        // 领域 ID（UUIDv7）
    provider_id: String,   // 协议 ID（provider 返回）
    name: String,
    index: usize,
    input: Value,
}
```

### ID 类型 API

每个类型（`ChatId` / `ChatTurnId` / `ToolCallId`）提供：
- `new_v7()`：生成 UUIDv7
- `parse_uuid7(str) -> Result<Self, IdParseError>`：只接受 version 7
- `from_legacy_or_new(str) -> Self`：历史兼容入口；非 UUIDv7 直接生成新 UUIDv7
- `as_uuid()` / `as_str()` / `Display`
- serde 序列化为 UUID 字符串，反序列化时严格检查 UUIDv7

### 核心规则

- 新会话中所有领域 ID **MUST** 为 UUIDv7（`new_v7()`）。
- Provider 返回的 tool id **MUST NOT** 作为领域 `ToolCallId`。
- 回填给 LLM 时 **MUST** 使用 `provider_id`，通过领域 `ToolCallId` 查找。
- 旧历史非 UUIDv7 id **MUST** 临时重新生成，不持久化兼容映射。
- 普通 serde 反序列化遇到非 UUIDv7 **MUST** 报错，防止新路径悄悄接受旧 id。

### Provider 消息边界

- `ContentBlock::ToolUse { id, ... }` 中的 `id` 继续表示 provider id
- `ContentBlock::ToolResult { tool_use_id, ... }` 继续表示 provider id
- provider conversion 不感知内部 UUIDv7，只处理 provider id

### 旧历史兼容

1. 如果 chat/turn/tool 内部 id 是 UUIDv7，直接解析
2. 如果不是 UUIDv7，生成新的 UUIDv7
3. 单次加载过程中 **MAY** 维护临时 in-memory 映射，确保同一旧 id 引用一致
4. 该映射 **MUST NOT** 持久化为"旧 id → 新 id"的全局兼容层
5. migration 后保存的新状态 **MUST** 只包含 UUIDv7

### 数据流

1. 用户输入开始新 chat：生成 `ChatId::new_v7()` + `ChatTurnId::new_v7()`
2. provider stream 收到 tool_use：`ToolIdentityRegistry` 分配/复用内部 `ToolCallId`，保存 `provider_id`
3. TUI 用 `chat_id + turn_id + tool_call_id` join timeline 与 tool payload
4. 回填 LLM：使用 result 中的 `provider_id` 构造 provider tool result message

## Agent Context 所有权

**project 拥有 workspace 的类型与规则，Runtime 仅持有实例生命周期。**

### 背景问题

原设计用 5 套类型表达同一组 workspace 事实，导致：
1. **所有权不清**：tools、project、runtime、session 都能重建同一组 workspace 字段
2. **撕裂读**：`workspace_root` 与 `path_base` 是两把独立 `Arc<Mutex>`，读者可能观察到中间态
3. **子 agent 共享 bug**：子 agent 经 `Arc` 克隆共享父 agent 的 workspace，`EnterWorktree` 会改到父 agent 的工作目录
4. **六边形违规**：worktree 业务规则直接内联 `std::process::Command::new("git")`

### 核心组件

**share 层**：
- `PersistedWorkspaceContext` / `PersistedWorkspaceFrame`：纯 serde DTO，仅用于会话持久化
- `WorkingContext` 移出 share，改为 project 内部的 `WorkspaceFrame`
- git 进程调用不进 share（`check-share-minimal-kernel.sh` 禁止）

**project 层**（workspace 切片 = 所有者）：
- `WorkspaceState { initial_cwd, workspace_root, path_base, stack }` —— 唯一可变 workspace 真相
- `WorkspaceFrame { path_base, workspace_root }` —— worktree 栈帧
- `WorkspaceService` —— 包 `Arc<Mutex<WorkspaceState>>`，**一把锁**，enter/exit 原子切换 root/base/stack

三个入站能力 trait（port）：
- `WorkspaceRead` = `current_workspace_root()` / `current_path_base()` / `resolve(rel)`
- `WorkspaceControl` = `set_cwd(path)` / `switch_to(path)` / `enter(path, branch)` / `exit()`
- `WorkspacePersist` = `snapshot()` / `restore(dto)`

出站端口：
- `GitWorktreeOps` —— trait 与默认实现 `GitCli` 均在 project；测试注入 `FakeGit`

**tools 层**：
- `ToolContext` → `ToolExecutionContext`：**删除** `workspace_root` / `path_base` / `context_stack` 三字段，改持有 `Arc<WorkspaceService>`
- 对外暴露窄访问器：`workspace_read() -> &dyn WorkspaceRead`（所有 tool）、`workspace_control() -> &dyn WorkspaceControl`（仅 bash + worktree 工具）

**runtime 层**：
- **删除** `ToolContextParts` 与 `build_tool_context`
- `WorkspaceService` 由 runtime client（`AgentClientImpl`）持有，跨 chat 轮次存活
- 子 agent：`parent_service.seed_isolated()` 造子实例（继承当前 root/base、空栈、新锁）

### 数据流

- **启动**：runtime client 构造并持有 `Arc<WorkspaceService>`（跨 chat 轮次存活）
- **工具批次**：runtime 用句柄构建 `ToolExecutionContext`
- **EnterWorktree**：工具 → `ctx.workspace_control().enter(path, branch)` → `WorkspaceService` 取锁一次 → 纯 `enter(&mut state, ...)` → 原子换 root/base
- **bash `cd`**：`ctx.workspace_control().set_cwd(path)` → 取锁一次 → 纯 `set_cwd(...)` → 经 `show_toplevel` 探测 root
- **session 保存**：`service.snapshot()` → `PersistedWorkspaceContext` → storage 落盘
- **session 恢复**：读 DTO → `service.restore(dto)` → 全校验后一次性替换

### 架构 Guard

- **R1** `ToolExecutionContext` 不得含 `workspace_root` / `path_base` / `context_stack` 字段
- **R2** tools 不得直接引用 `PersistedWorkspaceContext` 或 `WorkspacePersist`
- **R3** 仅 project 可定义 `WorkspaceState`
- **R4** 生产代码调 `.workspace_control()` 仅限 tools 的 `bash.rs` 与 `worktree.rs`
- **R5** 在 project 范围内，`Command::new("git")` 仅可出现在 `GitCli` adapter
- **R6** `WorkspacePersist` 仅可出现在 project 与 runtime


## Runtime Context 职责边界梳理

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
2. 本节已作为 `03-runtime-design.md` 的 "Runtime Context 职责边界" 章节落地；后续结构变更需同步更新本节。
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
## 参考文档

- [runtime 引擎规约](../specs/runtime.md)
- [UUIDv7 ID 设计](superpowers/specs/2026-06-13-runtime-tui-uuidv7-id-design.md)
- [Agent Context 所有权重构](superpowers/specs/2026-06-07-agent-context-ownership-redesign.md)
