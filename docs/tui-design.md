# TUI 设计

> 详细设计稿：[TUI Model/View 架构](superpowers/specs/2026-05-27-tui-model-view-architecture.md) · [TUI SDK DTO 边界](snapshot/specs/047-tui-sdk-dto-boundary-design.md)

## 定位

TUI 是**入站适配器**——负责把用户终端输入转换为核心域调用，把核心域事件转换为屏幕渲染。它不承载业务逻辑，不定义领域模型，不决定会话状态。

## 端口契约

TUI 通过 `AgentClient` trait（`packages/sdk`）与 Runtime 通信——这是核心域暴露的入站端口：

```
  Terminal Input                   Terminal Output
       │                                ▲
       ▼                                │
  ┌─────────────┐               ┌──────┴──────┐
  │  TUI        │               │  TUI        │
  │  Input      │               │  Render     │
  │  Adapter    │               │  Adapter    │
  └──────┬──────┘               └──────▲──────┘
         │ Msg                         │ ViewModel
         ▼                             │
  ┌─────────────┐               ┌──────┴──────┐
  │  Model      │──ViewAssembler──▶ ViewModel  │
  │  (业务真相)  │               │  (显示状态)  │
  └──────┬──────┘               └─────────────┘
         │ AgentClient trait
         ▼
  ┌─────────────┐
  │  Runtime    │
  │  (核心域)    │
  └─────────────┘
```

**六边形合规**：TUI 不直接调用 Runtime 内部类型，只依赖 `packages/sdk` 的 `AgentClient` trait 和 DTO。

## 分层数据流

```
Terminal Event → Msg → Coordinator / update → Model → ViewAssembler → ViewModel → Render → Effect
```

| 层 | 职责 | 六边形角色 |
|---|---|---|
| Msg | 统一入口，包装 terminal / Agent / timer / hook 输入 | 入站适配器转换 |
| Model | 按业务能力拆分的 Context，保存业务真相和状态转换规则 | 领域投影 |
| Intent | Coordinator 发给 Model Context 的处理意图 | 应用命令 |
| Change | Model Context 处理 Intent 后产生的状态变化事实 | 领域事件 |
| ViewAssembler | 从 Model + ViewState 组装 ViewModel | 投影组装 |
| ViewState | 纯显示交互状态（scroll / collapse / selection / animation） | 视图状态 |
| Render | 把 ViewModel + ViewState 画到 ratatui | 出站渲染 |
| Effect | update 后需要 Runtime 执行的副作用描述 | 出站副作用 |

## Model Context

Model 按业务能力拆分为四个 Context，对应核心域的不同投影：

| Context | 投影来源 | 职责 |
|---|---|---|
| Conversation | Session / Chat / Turn | 消息列表、tool call 状态、agent progress |
| Input | 用户终端 | 输入编辑、历史导航、自动补全、slash 命令识别 |
| Runtime | Session 状态 | 会话状态（idle / processing / waiting）、连接状态、cancel 信号 |
| Diagnostic | 成本 / token | 内部诊断信息（token 使用、成本、调试日志） |

## SDK DTO 边界

TUI 与 Runtime 的类型边界彻底消解——这是六边形架构的直接要求：

- `apps/cli/src/tui/**` **MUST NOT** 出现 `runtime::api` 或 `::runtime` 类型依赖。
- `sdk::ChatEvent` 使用强类型 DTO（`ToolResultImage` / `AgentProgressEventView` / `WorkspaceContextView` 等）。
- TUI 内部事件和渲染状态只使用 SDK DTO 或 TUI 私有 view model。
- Runtime 类型与 SDK DTO 的转换集中在 `agent/runtime` 的 `AgentClientImpl` 及 composition root。

## 关键约束

- `ToolCall.status` 是 tool 标题图标和颜色的唯一来源——TUI 不根据文本反推状态。
- 架构约束由 stop hook 保护，避免后续改动把业务逻辑塞回渲染层或 update 副作用中。
- TUI 作为入站适配器不定义领域模型，通过 `AgentClient` trait 与 Runtime 通信。
