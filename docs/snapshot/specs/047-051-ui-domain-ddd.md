# Feature #51：UI Domain DDD 设计 —— 将 apps/cli 提升为核心域

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/151

## 1. 设计目标

修正 #47 DDD 设计中对 "Interface" 的定位：app/cli 不是薄入口（Inbound Adapter），而是一个独立的 **UI Domain（核心域）**。本设计为 UI Domain 建立 Bounded Context 划分、统一语言和 Context Map。

目标：

1. 将 UI Domain 从 #47 的支撑域提升为核心域
2. 为 apps/cli 内部划分 Bounded Context
3. 定义 UI Domain 的统一语言
4. 定义 UI Domain 与 Agent Runtime 核心域之间的 Context Map
5. 设计 AgentClient SDK —— Agent Runtime 对外暴露的统一客户端接口
6. 定义 RunOrchestration 的 CLI 初始化流程（CLI 自己调用 AgentClient::new()）
7. 与 #50 CLI 目录整理形成从"物理收拢"到"逻辑边界"的衔接
8. 为后续 UI Widget 新增、Server 复用 UI 管线提供架构基线

非目标：

1. 不替代 #50 的目录迁移步骤
2. 不修改 agent/ 的 Bounded Context 划分
3. 不改变 TEA 架构（App/State/Msg/Cmd/Runtime）
4. 不引入新的 crate 拆分（本次只做设计基线，实施另行规划）

## 2. 核心域判断（修正 #47）

#47 将核心域定义为 Agent Runtime，Interface 列为支撑域：

| 能力 | #47 定位 |
|------|----------|
| Interface | TUI/REPL/AskUserQuestion 等输入输出适配层 |

**修正**：UI Domain 的复杂度（40+ 源文件、TEA 状态机、渲染管线、输入管线、Widget 协同）远超"适配层"的范畴。它有自己的状态模型、事件模型、领域规则和内部一致性约束。因此 UI Domain 应定义为 **第二个核心域**。

修正后：

| 域 | 定位 | 说明 |
|----|------|------|
| Agent Runtime | 核心域 | 不变：#47 定义的运行时核心 |
| **UI Domain** | **核心域** | TUI/REPL 的完整 UI 层，有自己的领域模型 |
| Provider | 支撑域 | 不变 |
| Tool | 支撑域 | 不变 |
| Project | 支撑域 | 不变 |
| Security / Policy | 支撑域 | 不变 |
| Audit | 支撑域 | 不变 |

核心逻辑：Agent Runtime 负责"要做什么"，UI Domain 负责"如何呈现和交互"。两者是对等的核心域，通过 **AgentClient SDK** 通信。

## 3. UI Domain 统一语言

### 3.1 核心术语

| 术语 | 定义 | 与 Agent Runtime 的关系 |
|------|------|-------------------------|
| **TuiApp** | UI Domain 的聚合根。持有所有 Widget、Event Loop、渲染调度器。 | 对应一次 `aemeath` 进程生命周期。 |
| **Widget** | UI Domain 中具有独立状态和渲染逻辑的 UI 单元。分为 4 类：input、display、status、popup。 | 通过 TEA Msg 与 TuiApp 通信。 |
| **Msg** | TEA 事件。包括 UI 事件（key/mouse/paste/resize）、Agent 事件（stream token/block/tool call）、Domain 事件（session 变更/压缩完成）。 | Msg 是 UI Domain 的通用事件语言。 |
| **Cmd** | TEA 副作用描述。所有 I/O、异步调用、hook 通知均通过 Cmd 描述，由 UI Runtime 执行。 | Cmd 是 UI Domain 与外部世界的唯一写通道。 |
| **Update** | 纯函数：`fn(&mut State, Msg) -> Option<Cmd>`。状态转移逻辑。 | 对应 Agent Runtime 的 Agent Looping，但作用于 UI 状态。 |
| **View (Render)** | 纯函数：`fn(&State) -> Frame`。将 State 映射为终端帧。 | UI Domain 内部，不依赖外部服务。 |
| **Display Pipeline** | 消息流 → block 拆分 → markdown 转换 → 语法高亮 → 增量渲染的完整管线。 | UI Domain 内部。 |
| **Interaction Pipeline** | 键鼠事件 → 输入缓冲 → 斜杠命令解析 → 自动补全 → Msg 派发的完整管线。 | UI Domain 内部。 |
| **Session UI Lifecycle** | Session 创建/恢复/暂停/压缩的 UI 侧状态管理。 | 通过 AgentClient 调起，但在 UI 侧有独立的生命周期状态。 |
| **Stream Buffer** | LLM 响应流的 UI 侧缓冲区。管理增量 token、block 边界检测、渲染节流。 | 对应 Agent Runtime 的 streaming 输出，UI Domain 做消费侧管理。 |
| **RunOrchestration** | 进程启动编排。解析 CLI 参数 → AgentClient::new() → 启动 TUI 或 REPL。 | UI Domain 的入口，连接 CLI 世界与 UI Domain。 |

### 3.2 需要避免的混淆

| 术语 | UI Domain 含义 | 不是 |
|------|---------------|------|
| State | TEA Model，UI 层的状态快照 | 不等同于 Agent Runtime 的 Session/Task 状态 |
| Session | UI 侧的 Session 上下文（当前会话 ID、标题、消息窗口） | 不等同于 Agent Runtime 的 Session |
| Runtime | UI Domain 的 Cmd 执行器，负责执行 Cmd 并产出 Msg | 不等同于 Agent Runtime |
| Event | TEA Msg，UI 领域事件 | 不等同于 Agent Runtime 的内部事件 |
| AgentClient | UI Domain 持有的 Agent Runtime 客户端 SDK | 不是 Agent 本身，是通信代理 |

## 4. Bounded Context 划分

UI Domain 内部识别出 4 个 Bounded Context + 1 个协调层：

```
┌─ UI Domain ───────────────────────────────────────────────────────────┐
│                                                                        │
│  ┌─ RunOrchestration (入口协调) ─┐                                    │
│  │ CLI args → AgentClient::new() → TuiApp                             │
│  └────────────────────────────────┘                                    │
│              │                                                         │
│  ┌───────────┴───────────┬───────────────┬──────────────────┐         │
│  │                        │               │                  │         │
│  ▼                        ▼               ▼                  ▼         │
│ ┌──────────┐    ┌──────────────┐  ┌──────────────┐  ┌─────────┐       │
│ │Display   │    │Interaction   │  │TEA Kernel    │  │Session  │       │
│ │(渲染)    │    │(输入交互)     │  │(事件驱动核)   │  │(生命周期)│       │
│ │          │    │              │  │              │  │         │       │
│ │• render  │    │• input_area  │  │• App/State   │  │• resume │       │
│ │• widgets │    │• completion  │  │• Msg/Cmd     │  │• create │       │
│ │• syntax  │    │• slash cmd   │  │• Update      │  │• compact│       │
│ │• diff    │    │• key/mouse   │  │• Event loop  │  │• save   │       │
│ │• markdown│    │• paste       │  │              │  │         │       │
│ │• theme   │    │• clipboard   │  │              │  │         │       │
│ │• status  │    │              │  │              │  │         │       │
│ └──────────┘    └──────────────┘  └──────────────┘  └─────────┘       │
│                                                                        │
└──────────────────────────────┬─────────────────────────────────────────┘
                               │
                      AgentClient SDK (唯一通道)
                               │
┌─ Agent Runtime Domain ───────┴─────────────────────────────────────────┐
│  AgentClient → Chat/Looping → Tool → Provider → ...                    │
│  AgentClient → Config/Client/Tooling/Hook/Runner/Session                │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.1 Display Context（渲染域）

**职责**：将 UI State 渲染为终端输出。管理所有视觉呈现逻辑。

**领域模型**：
- `Frame`：单帧渲染结果
- `Block`：可渲染的消息块（文本、工具调用、diff、progress）
- `Theme`：颜色/样式配置
- `StatusBar`：状态行模型
- `SyntaxHighlight`：语法高亮规则

**聚合根**：`Renderer`（持有当前 frame buffer、增量脏区）

**依赖**：
- 依赖 TEA Kernel 的 State（只读）
- 不依赖 AgentClient 或 Agent Runtime

**内聚规则**：
- Display Context 内可自由引用 `widgets/`、`syntax/`、`theme/`
- 禁止 Display Context 内的模块调用 AgentClient 或 `agent::` 模块
- 禁止 Display Context 直接读写文件系统（通过 Cmd 描述）

**对应 #50 目录**：`tui/display/` + `tui/widgets/`

### 4.2 Interaction Context（输入交互域）

**职责**：处理所有用户输入，将其转换为 UI 领域事件（Msg）。

**领域模型**：
- `InputBuffer`：多行输入缓冲区
- `Completion`：补全候选集
- `SlashCommand`：斜杠命令 AST
- `KeyBinding`：键位映射
- `PasteBuffer`：粘贴检测与批量处理

**聚合根**：`InputHandler`（持有当前输入状态、补全状态、命令解析器）

**依赖**：
- 产出 Msg 发送给 TEA Kernel
- 依赖 TEA Kernel 的 State（读取当前焦点 Widget、输入模式）
- 不依赖 AgentClient 或 Agent Runtime

**内聚规则**：
- Interaction Context 内可自由引用 `input_area/`、`completion/`、`slash/`
- 禁止直接调用 AgentClient 或 `agent::` 模块
- 剪贴板操作通过 Cmd 描述，不在 Interaction Context 内直接执行

**对应 #50 目录**：`tui/input/` + `tui/slash/` + `tui/completion/`

### 4.3 TEA Kernel Context（事件驱动核）

**职责**：UI Domain 的心脏。管理全局 State、Msg 路由、Update 逻辑、Cmd 队列、Event Loop。

**领域模型**：
- `App`：UI Domain 聚合根（持有所有 Widget、State、AgentClient）
- `State`：TEA Model（ChatState, InputState, LayoutState, AskUserState）
- `Msg`：TEA 事件（30+ 变体）
- `Cmd`：副作用描述
- `CmdExecutor`：Cmd 执行器（调用 AgentClient 或执行本地 I/O）

**聚合根**：`App`

**依赖**：
- 持有 `AgentClient`，通过 Cmd → CmdExecutor → AgentClient 访问 Agent Runtime
- 所有 Widget 依赖 Kernel 的 State 和 Msg
- Kernel 不依赖 Display/Interaction 的实现细节（通过 trait 解耦）

**内聚规则**：
- Kernel 只能通过 Msg 接收外部事件
- Kernel 只能通过 Cmd 发起副作用
- Kernel 的 `update()` 必须是纯函数
- 禁止 Kernel 内直接 `tokio::spawn`、文件 I/O、网络调用
- 禁止 Kernel 内其他代码直接调用 AgentClient（必须通过 CmdExecutor）

**对应 #50 目录**：`tui/core/`

### 4.4 Session Context（会话生命周期域）

**职责**：管理 UI 侧的 Session 生命周期——创建、恢复、暂停、压缩触发、持久化。

**领域模型**：
- `SessionHandle`：UI 侧的 Session 引用（session_id、title、message_count、token_usage）
- `SessionList`：可恢复的 Session 列表
- `CompactionTrigger`：压缩触发条件（token 阈值、消息数阈值）

**聚合根**：`SessionManager`

**依赖**：
- 依赖 AgentClient（通过 CmdExecutor 调用 Session 操作）
- 依赖 TEA Kernel（通过 Msg 通知状态变更）

**内聚规则**：
- Session Context 封装所有 Session 生命周期逻辑
- 其他 Context 不直接管理 Session 生命周期，通过 Session Context 的 API

**对应 #50 目录**：`tui/session/`

### 4.5 RunOrchestration（入口协调层）

**职责**：进程启动编排。解析 CLI 参数 → AgentClient 初始化 → 启动 TUI 或 REPL。

**职责范围**：
- 解析 CLI 参数（clap）
- 调用 `AgentClient::new(config, args)` 获取 AgentClient
- 决策启动 TUI 还是 REPL
- 注入初始 Msg（如恢复 Session 的消息历史）

**依赖**：仅依赖 `AgentClient::new()`

**对应 #50 目录**：`src/run_orchestration/`

## 5. Context Map

```
                       ┌──────────────┐
                       │RunOrchestration│
                       │  (入口协调)    │
                       └──────┬───────┘
                              │ AgentClient::new(config, args).await
                              │
              ┌───────────────┼───────────────┐
              │               │               │
              ▼               ▼               ▼
    ┌─────────────┐  ┌──────────────┐  ┌──────────────┐
    │Interaction  │  │  TEA Kernel  │  │   Session    │
    │  Context    │  │   Context    │  │   Context    │
    │             │  │              │  │              │
    │ (Input)     │  │ (App/State/  │  │ (Lifecycle)  │
    │             │  │  Msg/Cmd)    │  │              │
    └──────┬──────┘  └──────┬───────┘  └──────┬───────┘
           │                │                  │
           │ emits Msg      │ reads State      │ AgentClient
           │───────────────►│◄─────────────────│───────────┐
           │                │                  │           │
           │                │ reads State      │           │
           ▼                ▼                  │           │
    ┌─────────────┐                         │           │
    │  Display    │                         │           │
    │  Context    │                         │           │
    │             │                         │           │
    │ (Render)    │                         │           │
    └─────────────┘                         │           │
                                            ▼           ▼
                                   ┌──────────────────────────┐
                                   │    AgentClient SDK       │
                                   │    (唯一边界契约)          │
                                   ├──────────────────────────┤
                                   │  Agent Runtime Domain     │
                                   │  Chat/Looping/Tool/...    │
                                   └──────────────────────────┘
```

**关系说明**：

| 关系 | 上游 → 下游 | 类型 | 说明 |
|------|------------|------|------|
| Interaction → TEA Kernel | 用户事件 → Msg | 发布者/订阅者 | Interaction 产出 Msg，Kernel 消费 |
| TEA Kernel → Display | State → Frame | 共享内核 | Display 读取 State 渲染 |
| Session → TEA Kernel | Session 变更 → Msg | 发布者/订阅者 | Session 状态变更通知 UI |
| TEA Kernel → AgentClient | Cmd → SDK Call | 客户/供应商 | 唯一对外通信通道 |
| Session Context → AgentClient | Cmd → SDK Call | 客户/供应商 | Session 操作通过 AgentClient |
| Display → Agent Runtime | 禁止直接调用 | 防腐层 | Display 不感知 Agent Runtime |
| Interaction → Agent Runtime | 禁止直接调用 | 防腐层 | Interaction 不感知 Agent Runtime |

## 5b. Agent Runtime 侧修正：AgentClient SDK

### 问题

当前 `apps/cli/src/run_orchestration/setup.rs`（180 行）逐个调用 `agent::api::bootstrap` 的零散函数组装依赖，`run_orchestration/runtime.rs`（450 行）散落调用 `agent::api::chat::*`、`agent::api::session::*` 等。CLI 既在做编排又直接耦合到 Agent Runtime 内部模块。

两个问题：
1. **初始化编排泄漏**：CLI 逐个 build_* → 组装 → 注入，这是 Application 层逻辑被推到了 Adapter 层
2. **运行时 API 粒度过细**：CLI 直接调用 15+ 个内部 API 路径，没有统一的客户端契约

### 方案：AgentClient SDK

`AgentClient` 是 Agent Runtime 对外暴露的唯一客户端 SDK，CLI 自己调用 SDK 完成初始化，SDK 内部封装所有实现细节。

```rust
// AgentClient — 在 packages/sdk 中定义
pub struct AgentClient { /* 内部持有所有 Runtime 依赖 */ }

impl AgentClient {
    // === 工厂方法 ===
    pub async fn new(config: &Config, args: &CliArgs) -> Result<Self>;

    // === 只读视图（CLI 渲染用）===
    pub fn session(&self) -> &Session;             // messages 历史
    pub fn cost(&self) -> &CostTracker;            // token / 成本
    pub fn tasks(&self) -> &TaskStore;             // 任务列表
    pub fn project(&self) -> ProjectContext;       // 项目上下文（渲染状态栏）

    // === 对话 ===
    pub async fn chat(&self, input: ChatInput) -> Result<ChatStream>;
    pub fn cancel(&self);

    // === Session ===
    pub async fn save_session(&self) -> Result<()>;
    pub async fn load_session(&self, id: &SessionId) -> Result<Session>;
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>>;
    pub async fn delete_session(&self, id: &SessionId) -> Result<()>;

    // === 压缩 ===
    pub async fn compact(&self, session: &mut Session) -> Result<CompactResult>;
}
```

### 只读视图：ProjectContext

```rust
/// CLI 状态栏渲染所需的项目上下文聚合视图
/// 合并 WorkspaceContext（agent 侧）+ git branch（Shell 探测）
pub struct ProjectContext {
    pub path_base: String,        // 当前工作路径（相对）
    pub working_root: String,     // workspace 根目录
    pub worktree_kind: WorktreeKind, // Main / Worktree
    pub branch: Option<String>,   // git 分支
}
```

**为什么是 ProjectContext 而不是 WorkspaceContext**：

当前 CLI 的 `StatusLineContext` 和 Runtime 的 `WorkspaceContext` 各维护一份 `path_base` / `working_root`，造成双重追踪。`ProjectContext` 是 AgentClient 聚合后的统一视图——由 AgentClient 内部（`agent/runtime`）维护一份真实数据，CLI 只读不写。

**CLI 初始化（RunOrchestration）**：

```rust
// CLI 自己调用 SDK 初始化，不需要独立 BootstrapService
let config = ConfigManager::new(Some(&cwd)).load().await?;
let agent = AgentClient::new(&config, &args).await?;
let mode = agent.start_session(config.mode).await?;
match mode {
    ChatMode::Tui => TuiApp::new(agent).run(),
    ChatMode::NoTui => run_repl(agent),
}
```

| 对比 | 现状 | 目标 |
|------|------|------|
| CLI 感知 | 逐个 build_* + 15+ 内部 API 路径 | 只感知 `AgentClient` 一个类型 |
| 初始化 | 180 行 setup.rs 手动编排 | `AgentClient::new(config, args)` 一行 |
| 运行时 | 450 行 runtime.rs 零散 API 调用 | 通过 AgentClient 统一入口 |
| 渲染数据 | App 持有分散字段（SessionState、CmdExecutor、Skills）+ 18 个 run() 参数 | 4 个只读视图：`session()`、`cost()`、`tasks()`、`project()` |
| 归属 | 编排 + 胶水逻辑在 CLI | SDK 封装在 `packages/sdk`，实现细节在 `agent/runtime` |
| 编译期边界 | CLI 直接依赖 `agent::api::*` | CLI 只依赖 `packages/sdk` |

### 职责划分

| 模块 | 位于 | 职责 |
|------|------|------|
| `AgentClient`（trait + 类型） | `packages/sdk/src/client.rs` | 公开 SDK 接口。UI Domain / Server 等 Primary Adapter 编译期只依赖此 crate |
| `AgentClient::new()` 内部实现 | `agent/runtime/src/client_impl.rs` | `AgentClient` 构造器内部调用 build_* 完成初始化编排 |
| `build_*` 函数 | `agent/runtime/src/init/`，由 `AgentClient::new()` 内部调用 | 保持现有实现，变为 `pub(crate)` |
| `setup.rs` (CLI) | 大幅瘦身 | 保留 CLI 特有逻辑（参数补全），核心编排委托给 `AgentClient::new()` |

**依赖方向**：

```
apps/cli                     ← 只依赖 packages/sdk
    │
    ▼
packages/sdk                 ← AgentClient trait + 公共类型
    ▲
    │  implements
    │
agent/runtime                ← AgentClient 的真实实现 + 内部 build_*
```

### Context Map 更新

```
┌─ UI Domain ──────────────────┐
│                               │
│  RunOrchestration             │
│  let cfg = ConfigManager::load().await?;
│  let agent = AgentClient::new(&cfg, &args).await?;
│  let mode = agent.start_session(cfg.mode).await?;
│  match mode {
│    Tui => TuiApp::new(agent).run(),
│    NoTui => run_repl(agent),
│  }
└──────────────┬────────────────┘
               │ AgentClient (编译期: packages/sdk)
               │
┌─ packages/sdk ────────────────┐
│  AgentClient (trait + types)  │
│  ChatStream, SessionSummary, … │
└──────────────┬────────────────┘
               │ implements
┌─ agent/runtime ───────────────┐
│                               │
│  AgentClientImpl              │
│  ├─ new(config, args)         │
│  │   ├─ ConfigManager::load   │
│  │   ├─ resolve_api_key       │
│  │   ├─ build_llm_client      │
│  │   ├─ build_chat_tooling    │
│  │   ├─ build_hook_runner     │
│  │   ├─ build_agent_runner    │
│  │   └─ resolve_concurrency   │
│  ├─ chat(input) → ChatStream  │
│  ├─ save/load/list_session    │
│  ├─ compact(session)          │
│  └─ cost_tracker / tool_reg   │
│                               │
│  Provider / Storage / Tools   │
└───────────────────────────────┘
```

## 6. COLA 分层映射

将 COLA 分层应用到 UI Domain：

| COLA 层 | UI Domain 组件 | 说明 |
|---------|---------------|------|
| **Adapter** | `RunOrchestration` | CLI 参数 → AgentClient → UI Domain 入口 |
| **Application** | `TEA Kernel` (App, update, cmd_exec) | 编排 UI 业务流程 |
| **Domain** | `Display`, `Interaction`, `Session` | UI 领域模型和规则 |
| **Infrastructure** | `crossterm`, `ratatui`, `rustyline` | 终端 I/O 基础设施 |
| **Client** | `main.rs` | 进程入口 |

## 7. 与 #50 的衔接

#50（CLI TUI 目录整理）是本设计的物理层面的第一步。两者关系：

| #50 做的事 | 对应本设计的哪个 Context | 说明 |
|-----------|------------------------|------|
| `tui/core/` 收拢 TEA 核心 | **TEA Kernel Context** | 物理结构已对齐 |
| `tui/display/` 收拢渲染 | **Display Context** | 物理结构已对齐 |
| `tui/input/` 收拢输入 | **Interaction Context**（部分） | 还需将 slash/ 和 completion/ 纳入 Interaction |
| `tui/session/` 收拢生命周期 | **Session Context** | 物理结构已对齐 |
| `tui/slash/`、`tui/completion/` 独立 | **Interaction Context**（其余部分） | 应在概念上归属于 Interaction |
| `run_orchestration/` 收拢入口 | **RunOrchestration** | 物理结构已对齐 |

**#50 之后需要在概念层面做的事**（本次 spec 定义）：

1. **明确 Context 间依赖方向**：Interaction → Kernel ← Session → AgentClient；Display ← Kernel
2. **建立依赖守卫规则**：Display 禁止 import AgentClient 或 `agent::` 模块
3. **收拢 slash/ 和 completion/ 到 Interaction Context**（概念上，不必移动目录）
4. **定义各 Context 的公开 API**（pub mod 中只导出对外接口）
5. **更新 #47 spec 的 Context Map**，将 UI Domain 加入核心域，补充 AgentClient + BootstrapService

## 8. 依赖守卫规则

参照 #47 的架构守卫（7 项），UI Domain 内部新增以下守卫：

| # | 规则 | 检查方式 |
|---|------|---------|
| U1 | Display Context 禁止 `use agent::` / `use AgentClient` / 直接依赖 `packages/sdk` | grep 检查 |
| U2 | Interaction Context 禁止 `use agent::` / `use AgentClient` / 直接依赖 `packages/sdk` | grep 检查 |
| U3 | TEA Kernel 的 `update()` 禁止直接 `tokio::spawn` | grep 检查 |
| U4 | Display Context 只读 State，禁止 `&mut State` | 人工 review |
| U5 | 任意 Context 禁止绕过 CmdExecutor 直接调用 AgentClient。只有 CmdExecutor 能持有 AgentClient 引用 | grep 检查 |
| U6 | `tui/core/` 禁止 `use crate::tui::display::` | grep 检查 |
| U7 | `tui/core/` 禁止 `use crate::tui::input::` | grep 检查 |

## 9. 验证标准

1. 本 spec 与 #47 spec 的 Context Map 对齐（UI Domain 加入核心域，AgentClient trait 在 packages/sdk，Agent Runtime 通过 AgentClient 对外暴露）
2. apps/cli 的 `Cargo.toml` 中只依赖 `packages/sdk`，不直接依赖 `agent/`
3. `setup.rs` 瘦身为 `AgentClient::new(config, args)` 单行初始化
4. `runtime.rs`（450 行）中的零散 API 调用已迁移至 AgentClient
5. CmdExecutor 是 UI Domain 内唯一持有并调用 AgentClient 的位置
6. #50 全部 9 个 Phase 完成后，目录结构与 4 个 Context 的映射清晰
7. 依赖守卫规则 U1-U7 可在 CI 中自动化检查
8. 任何新增 Widget 能明确归属到 4 个 Context 之一

## 10. 后续实施建议

1. **Phase 0**（SDK）：在 `packages/sdk` 中定义 `AgentClient` trait 及公共类型，在 `agent/runtime` 中实现——封装 chat / session / compact 等全部运行时操作
2. **Phase 1**（CLI 初始化）：将 `setup.rs` 的 180 行 build_* 编排委托给 `AgentClient::new()`，CLI 瘦身为单行初始化
3. **Phase 2**（CLI 运行时）：将 `runtime.rs`（450 行）的零散 API 调用迁移至 AgentClient，CmdExecutor 成为唯一调用点
4. **Phase 3**（UI Domain）：完成 #50 CLI 目录整理（物理收拢）
5. **Phase 4**（UI Domain）：在 `tui/core/mod.rs`、`tui/display/mod.rs`、`tui/input/mod.rs`、`tui/session/mod.rs` 中添加 doc comment，标注所属 Context 和依赖规则
6. **Phase 5**（UI Domain）：实现依赖守卫脚本（U1-U7）
7. **Phase 6**（文档）：更新 #47 spec，将 UI Domain 纳入核心域，补充 AgentClient SDK 层
8. **Phase 7**（评估）：评估 Display Context 是否为 Server 复用做准备（markdown 渲染、diff 高亮可独立于 TUI）
