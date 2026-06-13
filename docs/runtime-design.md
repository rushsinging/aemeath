# Runtime 设计

> 对应 Issue: 核心域 Agent Runtime。
> 详细设计稿：[runtime 引擎规约](../specs/runtime.md) · [UUIDv7 ID 设计](superpowers/specs/2026-06-13-runtime-tui-uuidv7-id-design.md) · [Agent Context 所有权](superpowers/specs/2026-06-07-agent-context-ownership-redesign.md)

## 定位

Runtime 是核心域的**唯一应用服务**，所有入站适配器（TUI / CLI / Server）接入同一组 API。它不关心请求来自哪里——本地终端还是远端 WebSocket——只负责把一次用户输入推进成完整的 Agent 协作过程。

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

## 内部 ID 体系（UUIDv7）

内部实体 ID 与 provider 协议 ID 严格分离——核心域不依赖外部协议的 ID 约定：

| 层 | ID 类型 | 来源 | 用途 |
|---|---|---|---|
| 领域 | `ChatId` / `ChatTurnId` / `ToolCallId`（UUIDv7 newtype） | 核心域生成 | 跨 chat / turn / tool join |
| 协议 | `provider_id: String` | Provider 适配器返回 | 回填给 LLM 时使用 |
| 持久化 | 内部 UUIDv7 serde 为字符串 | Storage 适配器落盘 | 旧非 UUIDv7 id 加载时临时重新生成 |

**ToolCall 双 ID 结构**：

```rust
struct ToolCall {
    id: ToolCallId,        // 领域 ID（UUIDv7）
    provider_id: String,   // 协议 ID（provider 返回）
    // ...
}
```

核心规则：
- 新会话中所有领域 ID **MUST** 为 UUIDv7（`new_v7()`）。
- Provider 返回的 tool id **MUST NOT** 作为领域 `ToolCallId`。
- 回填给 LLM 时 **MUST** 使用 `provider_id`，通过领域 `ToolCallId` 查找。
- 旧历史非 UUIDv7 id **MUST** 临时重新生成，不持久化兼容映射。

ID 类型定义在 `packages/sdk`，由 Runtime / TUI 复用。

## Agent Context 所有权

workspace 状态是核心域的领域事实，通过端口暴露给入站适配器：

**project 拥有 workspace 的类型与规则，Runtime 仅持有实例生命周期。**

| 类型 | 归属 | 职责 |
|---|---|---|
| `WorkspaceService` | `features/project`（领域服务） | enter / exit worktree、git 校验，单一可变状态源 |
| `WorkspaceFrame` | `features/project`（内部） | 替代原 `WorkingContext`，不跨 feature |
| `PersistedWorkspaceContext` | `shared`（DTO） | 纯 serde DTO，仅用于会话持久化 |

收益：
- 消除撕裂读（单一 Mutex 保护整个 workspace 状态）。
- 子 agent 工作目录隔离（独立 `WorkspaceService` 实例）。
- 六边形合规（domain 不直接调 git，经 WorkspacePort / adapter）。

## Slash 命令系统

通过 `inventory` crate + 注册表自动收集。新增命令只需在 `core/command/commands/` 创建文件并用 `inventory::submit!` 声明，自动出现在 TUI 补全中。属于核心域的扩展点，不破坏六边形边界。
