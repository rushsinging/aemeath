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
2. **撕裂读**：`working_root` 与 `path_base` 是两把独立 `Arc<Mutex>`，读者可能观察到中间态
3. **子 agent 共享 bug**：子 agent 经 `Arc` 克隆共享父 agent 的 workspace，`EnterWorktree` 会改到父 agent 的工作目录
4. **六边形违规**：worktree 业务规则直接内联 `std::process::Command::new("git")`

### 核心组件

**share 层**：
- `PersistedWorkspaceContext` / `PersistedWorkspaceFrame`：纯 serde DTO，仅用于会话持久化
- `WorkingContext` 移出 share，改为 project 内部的 `WorkspaceFrame`
- git 进程调用不进 share（`check-share-minimal-kernel.sh` 禁止）

**project 层**（workspace 切片 = 所有者）：
- `WorkspaceState { initial_cwd, working_root, path_base, stack }` —— 唯一可变 workspace 真相
- `WorkspaceFrame { path_base, working_root }` —— worktree 栈帧
- `WorkspaceService` —— 包 `Arc<Mutex<WorkspaceState>>`，**一把锁**，enter/exit 原子切换 root/base/stack

三个入站能力 trait（port）：
- `WorkspaceRead` = `current_root()` / `current_path_base()` / `resolve(rel)`
- `WorkspaceControl` = `set_cwd(path)` / `switch_to(path)` / `enter(path, branch)` / `exit()`
- `WorkspacePersist` = `snapshot()` / `restore(dto)`

出站端口：
- `GitWorktreeOps` —— trait 与默认实现 `GitCli` 均在 project；测试注入 `FakeGit`

**tools 层**：
- `ToolContext` → `ToolExecutionContext`：**删除** `working_root` / `path_base` / `context_stack` 三字段，改持有 `Arc<WorkspaceService>`
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

- **R1** `ToolExecutionContext` 不得含 `working_root` / `path_base` / `context_stack` 字段
- **R2** tools 不得直接引用 `PersistedWorkspaceContext` 或 `WorkspacePersist`
- **R3** 仅 project 可定义 `WorkspaceState`
- **R4** 生产代码调 `.workspace_control()` 仅限 tools 的 `bash.rs` 与 `worktree.rs`
- **R5** 在 project 范围内，`Command::new("git")` 仅可出现在 `GitCli` adapter
- **R6** `WorkspacePersist` 仅可出现在 project 与 runtime

## 参考文档

- [runtime 引擎规约](../specs/runtime.md)
- [UUIDv7 ID 设计](superpowers/specs/2026-06-13-runtime-tui-uuidv7-id-design.md)
- [Agent Context 所有权重构](superpowers/specs/2026-06-07-agent-context-ownership-redesign.md)
