# Runtime 总设计

> 来源：[runtime 引擎规约](../specs/runtime.md)、[UUIDv7 ID 设计](superpowers/specs/2026-06-13-runtime-tui-uuidv7-id-design.md)、[Agent Context 所有权重构](superpowers/specs/2026-06-07-agent-context-ownership-redesign.md)

## Runtime 引擎

**Scope**：`agent/features/runtime/**`——Agent 主循环、tool 执行编排、token budget、对话压缩（compact）、成本追踪、slash 命令系统。

### Agent Looping

Agent Looping 是 Chat 内部的循环推进机制，协调：模型调用 → 工具执行 → SubAgent → 用户交互 → Task 更新 → 停止条件判断。

```
Session
  └── Chat
      └── Agent Looping
          ├── Main Turn
          ├── Child Turn*
          ├── ModelInvocation*
          ├── ToolExecution*
          ├── Task updates
          ├── AskUser pause/resume
          ├── Stop Hook
          └── Final response
```

### Tool 执行编排

LLM 返回 tool_use → Agent 收集 → 并发执行 → 结果注入回消息。`Tool` trait 与 `ToolRegistry` 定义在 `agent/features/tools`；runtime 只负责循环里的调度与结果回填。

### token budget / 压缩 / 成本

- token 估算：`runtime/src/business/compact/token_estimation.rs`
- 成本追踪与定价：`runtime/src/business/cost/pricing.rs`
- 成本历史落盘：`~/.agents/cost_history.json`
- 修改暂停/恢复/重试逻辑时 SHOULD 同步更新 `token_estimation`；成本逻辑更新时 SHOULD 同步更新 `pricing.rs`。

### slash 命令系统

通过 `inventory` crate + 注册表自动收集。新增命令只需在 `core/command/commands/` 创建文件并用 `inventory::submit!` 声明，自动出现在 TUI 补全中。

## 内部 ID 体系（UUIDv7）

内部实体 ID 与 provider 协议 ID 严格分离：

| 层 | ID 类型 | 来源 | 用途 |
|---|---|---|---|
| 内部 | `ChatId` / `ChatTurnId` / `ToolCallId`（UUIDv7 newtype） | runtime/TUI 生成 | 跨 chat/turn/tool join |
| Provider 协议 | `provider_id: String` | provider stream 返回 | 回填给 LLM 时使用 |
| 持久化 | 内部 UUIDv7 serde 为字符串 | 会话落盘 | 旧非 UUIDv7 id 加载时临时重新生成 |

### ToolCall 双 ID

```rust
struct ToolCall {
    id: ToolCallId,        // 内部 UUIDv7
    provider_id: String,   // provider 返回的 tool_use id
    ...
}
```

### 核心规则

- 新会话中所有内部 ID **MUST** 为 UUIDv7（`new_v7()`）。
- Provider 返回的 tool id **MUST NOT** 作为内部 `ToolCallId`。
- 回填给 LLM 时 **MUST** 使用 `provider_id`，通过内部 `ToolCallId` 查找。
- `ContentBlock::ToolUse.id` / `ToolResult.tool_use_id` 继续表达 provider 协议 ID。
- 旧历史非 UUIDv7 id **MUST** 临时重新生成，不持久化兼容映射。

ID 类型定义在 `packages/sdk`，由 runtime/TUI 复用。

## Agent Context 所有权

workspace 状态从 5 套类型收敛到单一 `WorkspaceService`：

**project 拥有 workspace 的类型与规则，runtime 仅持有实例生命周期。**

| 类型 | 所在 crate | 职责 |
|---|---|---|
| `WorkspaceService` | `agent/features/project` | enter/exit worktree、git 校验，单一可变状态源 |
| `WorkspaceFrame` | `agent/features/project`（内部） | 替代原 `WorkingContext`，不跨 crate |
| `PersistedWorkspaceContext` | `agent/shared` | 纯 serde DTO，仅用于会话持久化 |

### 依赖铁律

```
share   → ∅
project → share
tools   → share, project, storage
runtime → 全部 feature + share + sdk + logging
composition → runtime, tools, provider, project, sdk
```

无任何 feature 能依赖 runtime。git 进程调用不进 share（`check-share-minimal-kernel.sh` 禁止）。

### 收益

- 消除撕裂读（单一 `Mutex` 保护整个 workspace 状态）。
- 子 agent 工作目录隔离（独立 `WorkspaceService` 实例）。
- 六边形合规（domain 不直接调 git，经 port/adapter）。
