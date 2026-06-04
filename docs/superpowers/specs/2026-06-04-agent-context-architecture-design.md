# Agent Context 架构重构设计

## 背景

`agent/` 当前存在多套 context 类型表达相近的 workspace 事实：

- `ToolContext` 位于 `agent/features/tools/src/contract/context.rs`，包含工具执行态，也直接持有 `working_root`、`path_base`、`context_stack`。
- `ToolContextParts` 位于 runtime 的 chat loop 中，字段与 `ToolContext` 大量一一重复。
- `WorktreeWorkingContext` 位于 project worktree 业务层，再次包裹 `working_root`、`path_base`、`context_stack`。
- `WorkingContext` 位于 shared tool 类型中，表示 worktree stack 中的 `path_base` / `working_root` 快照。
- `WorkspaceContext` 位于 shared session DTO 中，使用 String 形式持久化同类信息。

这些重复让所有权不清晰：tools、project、runtime、session 都能看到或重建同一组 workspace 字段。长期会导致字段漂移、业务规则重复、子 agent 共享状态边界不明确，也让架构 guard 难以落地。

本设计目标是重做完整 Context 架构，优先架构纯净，允许调整内部 API 与模块边界。

## 架构选择

采用方案：**runtime 拥有运行态 context，feature 通过 consumer-visible port 访问能力；workspace 业务规则仍归 project domain**。

选择理由：

- runtime 实际拥有 session、agent loop、tool batch、子 agent、workspace restore/snapshot 的生命周期，因此 runtime 应持有运行时数据和编排状态。
- DDD / Clean / 六边形约束下，worktree 与 project path 的业务规则属于 project domain。runtime 不能把 git worktree 校验、同源 repo 判断、branch path sanitize、nested worktree 策略等 domain 规则上移成 runtime 业务。
- tools 和 project 不应持有完整运行时上下文，只应依赖自己需要的能力。
- shared 不应成为运行态大杂烩；shared 只保留跨 crate 的稳定 DTO、错误或轻量 contract。
- consumer-visible port 必须放在 consumer 可见但不反向依赖 runtime 的 crate 中，类比 `AgentRunner`。tools/project 绝不能依赖 runtime，否则会违反 `check-cargo-dependency-graph.sh` 的 DDD 依赖边界。
- 现有 `ProjectGateway` 已覆盖 project path/worktree/workspace context 能力。默认方向是演进 `ProjectGateway`，而不是新造同义 `WorkspaceAccess` port。

## 核心组件

### RuntimeContext

`RuntimeContext` 是 runtime 内部唯一会话级上下文根。它不放入 shared，也不被 tools/project 直接依赖。

职责：

- 拥有会话运行期状态。
- 初始化和恢复 workspace 状态。
- 为 tool batch 生成 `ToolExecutionContext`。
- 为子 agent 创建独立 runtime context 或 workspace seed。
- 在 session 保存时生成持久化快照。

建议内部拆分：

- `WorkspaceState`
- `ToolRuntimeState`
- `AgentRuntimeState`
- `SessionRuntimeState`
- `UiRuntimeState`

### WorkspaceState

`WorkspaceState` 是 runtime 内唯一可变 workspace 数据源。

它持有：

- `initial_cwd`
- 当前 `working_root`
- 当前 `path_base`
- `worktree_stack: Vec<WorkspaceFrame>`

职责：

- 维护当前 workspace 数据和 worktree stack。
- 通过 project domain port 执行 enter/exit worktree。
- 生成 session snapshot。
- 从 session DTO restore。

非职责：

- 不承载 git worktree domain 规则。
- 不直接判断同源 repo、branch path sanitize 或 nested worktree 策略。
- 不绕过 project domain 直接实现 `EnterWorktree` / `ExitWorktree` 业务。

这些规则应留在 project domain / `ProjectGateway` 背后。`WorkspaceState` 只负责持有 runtime 数据、调用 port、在成功后更新 runtime 状态。

### ToolExecutionContext

`ToolExecutionContext` 替代当前平铺式 `ToolContext`。它是一次 tool 调用或 tool batch 的执行视图，不拥有 workspace 状态。

它可以包含：

- `project_gateway: Arc<dyn ProjectGateway>`，或从 `ProjectGateway` 演进出的更窄 capability port。
- cancellation token。
- read-file tracker。
- plan mode。
- approval/permission 状态。
- progress sink。
- agent runner port。
- memory/reminder access。
- parent session id。

它不得直接包含 `working_root`、`path_base`、`context_stack` 字段。

### ProjectGateway / Workspace capability port

默认不新造同义 `WorkspaceAccess`。应优先演进现有 `ProjectGateway`，因为它已经是 project path、worktree transition 和 workspace context 的 OHS gateway。

目标：

- `ProjectGateway` 作为 project domain 规则的发布语言。
- runtime 持有 `Arc<dyn ProjectGateway>`，用它驱动 `WorkspaceState` 发生状态转换。
- tools 通过 `ToolExecutionContext` 获得所需的 project/workspace 能力，但不能依赖 runtime。
- `WorktreeWorkingContext` 三字段包装被退役，改为更窄的命令参数和 runtime state snapshot。
- `WorktreeContextExt` 投影随 `ToolContext` 平铺字段移除而退役。

`ProjectGateway` 演进方向：

- 将 `enter_worktree(ctx, path, branch)` 改为接收明确输入，例如当前 workspace snapshot + command args。
- 将 `exit_worktree(ctx)` 改为接收明确 stack/current state，而不是 `WorktreeWorkingContext` 包装。
- 将 `workspace_context_from_worktree_context` / `restore_workspace_context` 收束到 runtime session 边界，避免 tools 直接调用 persisted DTO 转换。
- 保持 project crate 只依赖 share，不依赖 runtime/tools。

只有当 `ProjectGateway` 演进后仍无法表达 tools 的 workspace 读能力时，才引入更窄的新 port。新 port 的定义位置必须满足：

- consumer 可见。
- 不造成 tools/project 依赖 runtime。
- 不与 `ProjectGateway` 形成同义双 port。

可选位置：

- `tools::contract`：适合 tool execution 视图，类比 `AgentRunner`。
- `project::api`：适合 project domain 发布语言。
- `share`：仅当 port 足够稳定、无 feature 语义、且不会让 shared 变成运行态容器时才可接受。

### WorkspaceFrame

`WorkspaceFrame` 替代当前 `WorkingContext`，表示 worktree stack 中的一个运行时快照。

字段：

- `path_base: PathBuf`
- `working_root: PathBuf`

### PersistedWorkspaceContext

`PersistedWorkspaceContext` 替代或重命名当前 `WorkspaceContext`，表示 session JSON DTO。

它只属于持久化边界，不参与 tool/project 执行路径。

建议保持 JSON 字段兼容：

- `path_base: String`
- `working_root: String`
- `context_stack: Vec<PersistedWorkspaceFrame>`

命名上必须明确它是 persisted DTO，不是 runtime context。

## 数据流

### 启动 / 新会话

runtime 创建 `RuntimeContext`。`RuntimeContext::new()` 初始化 `WorkspaceState`：

- `initial_cwd`
- `working_root`
- `path_base`
- 空 `worktree_stack`

其他运行态同时初始化。

### 工具调用

agent loop 不再手动拼 `ToolContextParts`。每次 tool batch 前，由 `RuntimeContext` 生成 `ToolExecutionContext`。

工具只能通过 context 中的能力接口访问 workspace。文件、搜索、bash、worktree 等工具不得直接 lock 或持有 workspace 三字段。

`ToolExecutionContext` 可以持有 project/workspace capability port，但 port 定义不能位于 runtime。tools 依赖方向必须继续满足 `check-cargo-dependency-graph.sh`：tools 可经已批准边界依赖 project/share/storage，但不得依赖 runtime。

### EnterWorktree / ExitWorktree

tool crate 只负责：

- 解析输入参数。
- 调用 `ProjectGateway` 或由 `ProjectGateway` 演进出的更窄 worktree capability。
- 将成功或失败映射为 `ToolResult`。

实际业务规则由 project domain 负责：

- path resolve。
- git repo 校验。
- 同源 repo 判断。
- branch path sanitize。
- nested worktree 拒绝或栈残留修复。
- 计算 enter/exit 后的新 workspace frame/current state。

runtime 的 `WorkspaceState` 在 project domain 返回成功结果后更新自身数据：

- push/pop `WorkspaceFrame`。
- 切换 `path_base` / `working_root`。
- 触发 session/TUI 所需的 workspace changed event。

project crate 不再自建 context struct 包裹同一组三字段。它暴露 domain operation 和 DTO，runtime 负责持有运行时状态。

### Session snapshot / restore

保存 session 时：

1. runtime 调 `RuntimeContext::workspace_snapshot()`。
2. `WorkspaceState` 转成 `PersistedWorkspaceContext`。
3. session 存储 DTO。

恢复 session 时：

1. runtime 读取 `PersistedWorkspaceContext`。
2. `WorkspaceState::restore(dto)` 校验路径。
3. 校验通过后一次性替换 runtime workspace 状态。

DTO 不进入 tool/project 执行路径。

### 子 agent

子 agent 拥有自己的 `RuntimeContext`，但不是所有 runtime state 都应隔离。创建子 agent 时必须按状态类别显式分类，避免 workspace 隔离后破坏全局限流或审计一致性。

| 状态 | 默认策略 | 理由 |
|---|---|---|
| workspace current state / stack | 隔离，可由 snapshot seed 初始化 | 子 agent enter/exit worktree 不得影响父 agent |
| conversation/model/session transcript | 隔离 | 子 agent 是独立执行上下文 |
| `agent_semaphore` / 全局 agent 并发限流 | 共享 | 每个子 agent 各自创建 semaphore 会导致全局限流失效 |
| cancellation token | 继承父级取消 + 子级本地取消 | 父级中断应能停止子 agent；子级取消不应误停父级 |
| `read_files` tracker | 默认共享，除非明确改为分层 tracker | 读文件 guard、Edit 前 Read 约束和审计应跨父子 agent 保持一致 |
| permission / approval / allow_all | 继承策略，不共享可变决策缓存 | 保持权限语义一致，避免子 agent 私自扩大权限 |
| progress sink | 桥接到父级 tool call | 子 agent 进度要回传父级 TUI/tool result |
| session reminders | 按会话语义共享或桥接 | 父级需要看到 hook/stop 提醒；不能丢失提醒 |
| memory config | 共享只读配置 | 配置一致即可，无需共享 mutable runtime state |
| hook runner project dir | 随子 agent workspace 派生 | hook 环境必须匹配子 agent 当前 working root |

父 agent 创建子 agent 时，显式决定是否继承 workspace：

- 不继承：子 agent 使用自己的启动 cwd 初始化。
- 继承：父 agent 传递 `PersistedWorkspaceContext`、`WorkspaceSeed` 或 project domain snapshot。

子 agent 不共享父 agent 的 workspace mutex。共享项必须是有意设计的共享能力，例如 `agent_semaphore`，不能通过 clone 整个旧 `ToolContext` 隐式共享。

## 迁移策略

虽然目标是架构纯净，实施仍分阶段，避免一次性大爆炸。

1. **演进 ProjectGateway 并引入 RuntimeContext 外壳**
   - 先审计 `ProjectGateway` 当前职责，确认哪些方法属于 project domain，哪些只是 runtime state 投影。
   - 新增 `RuntimeContext`、`WorkspaceState`、`WorkspaceFrame`。
   - 保留旧 `ToolContext` 兼容层。
   - 不先新增同义 `WorkspaceAccess`；除非 `ProjectGateway` 演进后仍无法表达必要能力。

2. **迁移 worktree 工具**
   - `EnterWorktree` / `ExitWorktree` 改走演进后的 `ProjectGateway` 或更窄 worktree capability。
   - 删除 `WorktreeWorkingContext` 或仅保留短期兼容别名。
   - 同步退役 `WorktreeContextExt` 投影。

3. **迁移路径相关工具**
   - file read/edit/write、glob、grep、bash 等统一通过 `ToolExecutionContext` 暴露的能力获取 base/root。
   - 不再读取 `ToolContext.path_base` / `ToolContext.working_root`。

4. **重塑 ToolContext**
   - 将 `ToolContext` 重命名或重塑为 `ToolExecutionContext`。
   - 删除 `working_root`、`path_base`、`context_stack` 字段。
   - 删除 `ToolContextParts`，改由 `RuntimeContext::tool_execution_context()` 生成。
   - 子 agent 创建路径按“隔离 vs 共享”状态表显式传递能力，禁止 clone 整个旧 context 作为默认策略。

5. **整理持久化 DTO**
   - `WorkspaceContext` 迁移为 `PersistedWorkspaceContext`。
   - 保持 serde 字段兼容。
   - 转换只发生在 session 边界。

6. **迁移 guard 豁免名单**
   - `check-crate-api-boundary.sh` 当前已对 `WorktreeContextExt` / `WorktreeWorkingContext` / workspace context 投影登记豁免。
   - 每个迁移阶段必须同步更新豁免名单：旧投影仍存在时保留旧豁免；新 port 引入时增加精确豁免；旧投影删除时移除旧豁免。
   - 豁免必须阶段化、命名化，不能永久扩大。

7. **收尾**
   - 删除兼容别名和旧转换函数。
   - 更新 docs/specs 中的架构规则。
   - 补齐架构 guard、单元测试和回归验证。

## 错误处理

workspace / project 错误集中到 project domain 与 runtime workspace 边界。project domain 负责 worktree/path 业务错误；runtime workspace 层负责 session restore、状态替换和 mutex/并发边界错误。可新增 `WorkspaceError` / `ProjectWorkspaceError`，或接入现有 `AemeathError` 体系。

覆盖场景：

- 路径不存在。
- path 不在允许工作区内。
- git worktree 创建失败。
- worktree 归属仓库不一致。
- context stack 为空。
- session restore 路径失效。
- session restore stack frame 无效。
- mutex poison 后无法恢复或需要降级。

用户可见错误保持中文。tool 层不拼复杂业务错误，只把 project/runtime workspace 边界错误映射为 `ToolResult`。

session restore 遇到无效 path 或无效 stack 时整体失败，避免半恢复状态。

## 测试策略

### WorkspaceState 单元测试

覆盖：

- 初始化时 cwd/root/base 一致。
- 相对路径基于 `path_base` 解析。
- 绝对路径按原样处理。
- enter push 当前 frame。
- exit pop 并恢复。
- 空 stack exit 报错。
- snapshot/restore 对称转换。
- restore 无效路径整体失败。

### Worktree 业务测试

git 命令相关逻辑应尽量隔离到 adapter/port。单测优先覆盖纯逻辑：

- branch path sanitize。
- same repo 判断结果处理。
- nested worktree 拒绝策略。
- stack 残留修复策略。

必要时增加真实 git worktree 集成测试。

### ToolExecutionContext 测试

覆盖：

- 生成 context 时不暴露重复 workspace 字段。
- tool 通过 `ProjectGateway` 或演进出的更窄 capability port 获取路径。
- plan mode、allow_all、progress、read_files、agent runner、session reminders 继续传递。

### Session 测试

覆盖：

- 保存 session 时包含 `PersistedWorkspaceContext`。
- 恢复 session 后 workspace 状态一致。
- 旧 JSON 字段兼容。

### 子 agent 隔离测试

覆盖：

- 父 agent enter worktree 后创建子 agent。
- 子 agent 使用 snapshot 初始化。
- 子 agent 不共享父 agent mutex。
- 子 agent 切换 worktree 不影响父 agent。

### 回归验证

实施阶段至少运行：

- `cargo test -p runtime`
- `cargo test -p tools`
- `cargo test -p project`
- `cargo check`

必要时运行 TUI/SDK 相关测试，确认工具事件和 session restore 没有破坏。

## 架构 Guard 与 Stop hook 联动

这次重构必须增加架构 guard，防止未来重新退回字段平铺和 context 重叠。

### Guard 规则

建议检查：

- `ToolExecutionContext` 不允许包含 `working_root`、`path_base`、`context_stack` 字段。
- tools 不允许直接引用 persisted workspace DTO。
- tools/project 不允许依赖 runtime；任何新 port 的 crate 位置必须通过 `check-cargo-dependency-graph.sh`。
- project 不允许重新定义包裹 `working_root`、`path_base`、`context_stack` 的 context struct。
- `WorkspaceState` 是唯一 runtime mutable workspace data source，但 worktree domain 规则必须留在 project / `ProjectGateway`。
- `EnterWorktree` / `ExitWorktree` 必须通过 `ProjectGateway` 或其演进出的 project domain capability，不能绕过 project domain 直接改状态。
- 不允许在 `ProjectGateway` 与新 `WorkspaceAccess` 之间长期保留同义双 port。
- 子 agent 创建路径必须显式分类共享/隔离状态，不允许简单 clone 全量旧 context 作为最终设计。

### 执行入口

项目已经在 `.agents/aemeath.json` 配置 Stop hook：

- `hooks.Stop[0].command = "{AEMEATH_PROJECT_DIR}/.agents/hooks/check-architecture-guards.sh"`

因此 context 架构 guard 应接入现有脚本：

- `.agents/hooks/check-architecture-guards.sh`

Stop hook 只负责调用统一 guard，不内嵌复杂 Rust/源码解析逻辑。

### 触发范围

`check-architecture-guards.sh` 可在 diff 涉及以下路径时运行 context guard：

- `agent/features/runtime/**`
- `agent/features/tools/**`
- `agent/features/project/**`
- `agent/shared/src/session_types.rs`
- `agent/shared/src/tool.rs`

未涉及相关路径时可以跳过，以降低 Stop hook 成本。

### Guard 实现方式

短期推荐实现为 Rust 架构测试或轻量 check 命令，例如：

- `cargo test -p runtime context_architecture_guard`

中期如项目引入 xtask，可迁移为：

- `cargo xtask arch-check context`

已有 guard 必须一起维护：

- `check-cargo-dependency-graph.sh`：确认 tools/project 不依赖 runtime，且任何新 crate edge 都有明确白名单。
- `check-crate-api-boundary.sh`：迁移期同步维护 `WorktreeContextExt` / `WorktreeWorkingContext` / 新 port 的精确豁免名单。

原则：

- CI、本地手动验证、Stop hook 复用同一检查入口。
- hook 不复制规则，避免规则漂移。
- guard 失败消息必须说明违反了哪条架构约束和建议修复方向。
- 迁移豁免必须随阶段收缩；旧投影删除后必须删除对应豁免。

## 非目标

本设计不包含：

- 改变 session JSON 字段语义或破坏旧 session 兼容。
- 重做 permission/policy/hook 全体系。
- 改变 tool schema 或 TUI 展示协议。
- 引入跨进程 workspace 状态共享。

## 验收标准

实施完成后应满足：

- 运行时 workspace 可变数据只有 `WorkspaceState` 一个 owner。
- worktree / project path 业务规则保留在 project domain / `ProjectGateway`，runtime 只持有数据和编排调用。
- `ToolExecutionContext` 不再平铺 workspace 三字段。
- tools/project 通过 consumer-visible capability port 访问 workspace，且不得依赖 runtime。
- 优先演进 `ProjectGateway`，不引入与其长期同义的 `WorkspaceAccess`。
- `WorktreeWorkingContext` 三字段包装和 `WorktreeContextExt` 投影被退役，相关 guard 豁免同步收缩。
- session persisted DTO 与 runtime state 边界清晰。
- 子 agent workspace 隔离，但 `agent_semaphore` 等必须共享的 runtime capability 仍按分类规则共享。
- 架构 guard 接入 `.agents/hooks/check-architecture-guards.sh`，Stop hook 可阻止违反 context 架构规则的变更。
- 相关单元测试、架构测试和 `cargo check` 通过。
