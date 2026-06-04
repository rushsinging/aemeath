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

采用方案：**runtime 拥有 context，feature 通过能力接口访问**。

选择理由：

- runtime 实际拥有 session、agent loop、tool batch、子 agent、workspace restore/snapshot 的生命周期。
- tools 和 project 不应持有完整运行时上下文，只应依赖自己需要的能力。
- shared 不应成为运行态大杂烩；shared 只保留跨 crate 的稳定 DTO、错误或轻量 contract。
- project 不适合作为完整 context owner，因为 plan mode、read-file tracker、agent runner、progress sink、session reminders 等执行态属于 runtime/tool 编排。

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

`WorkspaceState` 是 runtime 内唯一可变 workspace 状态源。

它持有：

- `initial_cwd`
- 当前 `working_root`
- 当前 `path_base`
- `worktree_stack: Vec<WorkspaceFrame>`

职责：

- 解析相对/绝对路径。
- 维护 worktree enter/exit stack。
- 校验 worktree 路径、git repo 归属和嵌套进入规则。
- 生成 session snapshot。
- 从 session DTO restore。

所有修改当前 workspace 的路径都必须经过 `WorkspaceState` 或它导出的能力接口。

### ToolExecutionContext

`ToolExecutionContext` 替代当前平铺式 `ToolContext`。它是一次 tool 调用或 tool batch 的执行视图，不拥有 workspace 状态。

它可以包含：

- `workspace: Arc<dyn WorkspaceAccess>` 或更窄能力接口。
- cancellation token。
- read-file tracker。
- plan mode。
- approval/permission 状态。
- progress sink。
- agent runner port。
- memory/reminder access。
- parent session id。

它不得直接包含 `working_root`、`path_base`、`context_stack` 字段。

### WorkspaceAccess

`WorkspaceAccess` 是 tools/project 访问 workspace 的能力接口。

建议能力：

- 读取当前 path base。
- 读取当前 working root。
- 基于 path base 解析工具路径。
- 进入 worktree。
- 退出 worktree。
- 生成 persisted snapshot。
- 从 persisted snapshot restore。

根据依赖方向，可再拆出更窄接口，例如：

- `WorkspaceReadAccess`：文件/search/bash 等只读当前路径信息。
- `WorktreeAccess`：EnterWorktree/ExitWorktree 专用。
- `WorkspacePersistenceAccess`：session snapshot/restore 专用。

拆分原则是 feature 只拿它实际需要的能力。

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

### EnterWorktree / ExitWorktree

tool crate 只负责：

- 解析输入参数。
- 调用 `WorktreeAccess` / `WorkspaceAccess`。
- 将成功或失败映射为 `ToolResult`。

实际业务逻辑由 workspace 层负责：

- path resolve。
- git repo 校验。
- 同源 repo 判断。
- nested worktree 拒绝或栈残留修复。
- push/pop `WorkspaceFrame`。
- 切换 `path_base` / `working_root`。

project crate 可以保留纯业务函数，但输入应是明确参数或能力 trait，不再自建 context struct 包裹同一组三字段。

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

子 agent 拥有自己的 `RuntimeContext`。

父 agent 创建子 agent 时，显式决定是否继承 workspace：

- 不继承：子 agent 使用自己的启动 cwd 初始化。
- 继承：父 agent 传递 `PersistedWorkspaceContext` 或 `WorkspaceSeed`。

子 agent 不共享父 agent 的 `Arc<Mutex<_>>` workspace 状态。子 agent enter/exit worktree 不应影响父 agent。

## 迁移策略

虽然目标是架构纯净，实施仍分阶段，避免一次性大爆炸。

1. **引入新类型**
   - 新增 `RuntimeContext`、`WorkspaceState`、`WorkspaceFrame`、`WorkspaceAccess`。
   - 保留旧 `ToolContext` 兼容层。

2. **迁移 worktree 工具**
   - `EnterWorktree` / `ExitWorktree` 改走 `WorkspaceAccess`。
   - 删除 `WorktreeWorkingContext` 或仅保留短期兼容别名。

3. **迁移路径相关工具**
   - file read/edit/write、glob、grep、bash 等统一通过 `WorkspaceAccess` 获取 base/root。
   - 不再读取 `ToolContext.path_base` / `ToolContext.working_root`。

4. **重塑 ToolContext**
   - 将 `ToolContext` 重命名或重塑为 `ToolExecutionContext`。
   - 删除 `working_root`、`path_base`、`context_stack` 字段。
   - 删除 `ToolContextParts`，改由 `RuntimeContext::tool_execution_context()` 生成。

5. **整理持久化 DTO**
   - `WorkspaceContext` 迁移为 `PersistedWorkspaceContext`。
   - 保持 serde 字段兼容。
   - 转换只发生在 session 边界。

6. **收尾**
   - 删除兼容别名和旧转换函数。
   - 更新 docs/specs 中的架构规则。
   - 补齐架构 guard、单元测试和回归验证。

## 错误处理

workspace 错误集中到 workspace 层。可新增 `WorkspaceError`，或接入现有 `AemeathError` 体系。

覆盖场景：

- 路径不存在。
- path 不在允许工作区内。
- git worktree 创建失败。
- worktree 归属仓库不一致。
- context stack 为空。
- session restore 路径失效。
- session restore stack frame 无效。
- mutex poison 后无法恢复或需要降级。

用户可见错误保持中文。tool 层不拼复杂业务错误，只把 workspace 层错误映射为 `ToolResult`。

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
- tool 通过 `WorkspaceAccess` 获取路径。
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
- project 不允许重新定义包裹 `working_root`、`path_base`、`context_stack` 的 context struct。
- `WorkspaceState` 是唯一 runtime mutable workspace source。
- `EnterWorktree` / `ExitWorktree` 必须通过 `WorkspaceAccess`，不能绕过 workspace 层直接改状态。

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

原则：

- CI、本地手动验证、Stop hook 复用同一检查入口。
- hook 不复制规则，避免规则漂移。
- guard 失败消息必须说明违反了哪条架构约束和建议修复方向。

## 非目标

本设计不包含：

- 改变 session JSON 字段语义或破坏旧 session 兼容。
- 重做 permission/policy/hook 全体系。
- 改变 tool schema 或 TUI 展示协议。
- 引入跨进程 workspace 状态共享。

## 验收标准

实施完成后应满足：

- 运行时 workspace 可变状态只有 `WorkspaceState` 一个 owner。
- `ToolExecutionContext` 不再平铺 workspace 三字段。
- tools/project 通过能力接口访问 workspace。
- session persisted DTO 与 runtime state 边界清晰。
- 子 agent 不共享父 agent workspace mutex。
- 架构 guard 接入 `.agents/hooks/check-architecture-guards.sh`，Stop hook 可阻止违反 context 架构规则的变更。
- 相关单元测试、架构测试和 `cargo check` 通过。
