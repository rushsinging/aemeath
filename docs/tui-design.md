# TUI 总设计

> 来源：[TUI Model/View 架构设计](superpowers/specs/2026-05-27-tui-model-view-architecture.md)、[TUI SDK DTO 边界设计](feature/specs/047-tui-sdk-dto-boundary-design.md)

## 设计目标

TUI 的核心模型是"用户与 Agent 的交互会话"，不是 `status line` / `input area` / `output area` 三个屏幕区域。

1. 业务真相只存在于 Model State；View State 只服务显示，不能反向决定业务状态。
2. 保留 TEA 外壳，用 Model Context 重构 TEA Model 内部边界。
3. Agent/SDK/runtime 事件进入 TUI 后必须先被适配为内部意图，不能直接修改输出行。
4. Render 只消费 ViewModel 和 ViewState，不匹配 tool id、不修改模型、不根据文本反推状态。

## 分层数据流

```
External Event → Msg → Application Coordinator / update → Model → ViewAssembler → ViewModel → Render → Effect
```

| 层 | 职责 |
|---|---|
| Msg | TEA update loop 统一入口，包装 terminal / Agent / timer / hook 等外部输入 |
| Model | 按业务能力拆分的 Context（Conversation / Input / Runtime / Diagnostic），保存业务真相和状态转换规则 |
| Intent | Application Coordinator 发给某个 Model Context 的处理意图 |
| Change | Model Context 处理 Intent 后产生的状态变化事实 |
| ViewAssembler | 从 Model + ViewState 组装 ViewModel |
| ViewState | 纯显示交互状态（scroll / collapse / selection / animation / render cache） |
| Render | 把 ViewModel + ViewState 画到 terminal 的 ratatui 层 |
| Effect | update 后需要 runtime 执行的副作用描述 |

## SDK DTO 边界

TUI 与 runtime 的类型边界彻底消解：

- `apps/cli/src/tui/**` 不出现 `runtime::api` 或 `::runtime` 类型依赖。
- `sdk::ChatEvent` 使用强类型 SDK DTO（`ToolResultImage` / `AgentProgressEventView` / `WorkspaceContextView` 等）。
- TUI 内部事件和渲染状态只使用 SDK DTO 或 TUI 私有 view model。
- runtime 类型与 SDK DTO 的转换集中在 `agent/runtime` 的 `AgentClientImpl` 及 composition root。

## Model Context

| Context | 职责 |
|---|---|
| Conversation | 消息列表、tool call 状态、agent progress，维护对话真相 |
| Input | 用户输入编辑、历史导航、自动补全、slash 命令识别 |
| Runtime | 会话状态（idle / processing / waiting）、连接状态、cancel 信号 |
| Diagnostic | 内部诊断信息（token 使用、成本、调试日志） |

## 关键约束

- `ToolCall.status` 是 tool 标题图标和颜色的唯一来源。
- 架构约束由 stop hook 保护，避免后续改动把业务逻辑塞回渲染层或 update 副作用中。
- 不再使用 `Domain` / `Projection` / `Presenter` / `Cmd` 作为目标架构术语。
- TUI 作为 CLI Adapter 不定义 Domain Model，通过 `AgentClient` trait（`packages/sdk`）与 Runtime 通信。
