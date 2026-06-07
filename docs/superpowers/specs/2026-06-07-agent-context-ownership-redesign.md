# Agent Context 所有权重构设计(project 拥有 WorkspaceState)

## 背景

`agent/` 当前用 5 套类型表达同一组 workspace 事实,所有权弥散:

- `ToolContext`(`agent/features/tools/src/contract/context.rs`)平铺持有 `working_root`、`path_base`、`context_stack` 三个 `Arc<Mutex<…>>`。
- `ToolContextParts`(`agent/features/runtime/src/business/chat/looping/tool_context.rs`)字段与 `ToolContext` 大量重复。
- `WorktreeWorkingContext`(`agent/features/project/src/business/worktree.rs`)再次包裹同三字段。
- `WorkingContext`(`agent/shared/src/tool.rs`)是 worktree 栈帧快照。
- `WorkspaceContext`(`agent/shared/src/session_types.rs`)是 String 形式的 session 持久化 DTO。

由此产生四类问题:

1. **所有权不清**:tools、project、runtime、session 都能看到或重建同一组 workspace 字段,易字段漂移、业务规则重复。
2. **撕裂读**:`working_root` 与 `path_base` 是两把独立的 `Arc<Mutex>`,enter/exit 分别加锁,读者可能观察到 root 已切、base 未切的中间态。
3. **子 agent 共享 bug**:子 agent 经 `Arc` 克隆共享父 agent 的 workspace 三字段,子 agent `EnterWorktree` 会改到父 agent 的工作目录。
4. **六边形违规**:worktree 业务规则(`project/src/business/worktree.rs`)直接内联 `std::process::Command::new("git")`,domain 直捅 infra。

本设计在 `2026-06-04-agent-context-architecture-design.md` 基础上修正其"runtime 拥有 context"的归属判断:经一手核实依赖图后,改为 **project 拥有 workspace 的类型与规则,runtime 仅拥有实例生命周期**。

## 架构选择

抛开"runtime 拥有 context"。采用:**project 切片拥有 workspace 状态容器与转换规则;runtime 持有该 project 所有物的 `Arc<WorkspaceService>` 句柄。**

依据实测依赖图(铁律:无任何 feature 能依赖 runtime):

```
share   → ∅                         sdk → ∅
project → share                     provider/prompt/storage → share
tools   → share, project, storage   ← 唯一横向:tools 可用 project
runtime → 全部 feature + share + sdk + logging
composition → runtime,tools,provider,project,sdk
```

理由:

- workspace/worktree 本就是 project 的领域,enter/exit/git 校验规则已在 `project/src/business/worktree.rs`,且 project 只依赖 share(最内层)——规则留此符合 Clean 依赖规则与 VSA 切片自治。
- tools 已被允许依赖 project,可直接经 `project::api` 消费 workspace,无需绕道 runtime。
- runtime 不再**定义** context,只持有句柄并编排生命周期,从而解除"runtime 拥有 context"的约束:**类型 + 规则归 project,实例生命周期归 runtime** 是两件事。
- 持久化 DTO 留 share(跨持久化边界被 project 与 runtime/storage 引用)。

## 核心组件

### share(收缩,更贴 minimal-kernel)

- `WorkspaceContext` → 重命名 **`PersistedWorkspaceContext`**;`WorkspaceStackEntry` → **`PersistedWorkspaceFrame`**。serde 字段不变(`path_base` / `working_root` / `context_stack`),保持旧 session 兼容。仅作持久化 DTO,不进 tool/project 执行路径。
- **`WorkingContext` 移出 share**:它是运行期栈帧,重构后不再跨 crate,改为 project 内部的 `WorkspaceFrame`。
- **git 进程调用不进 share**:`check-share-minimal-kernel.sh` 禁止 share 出现 `Command::new`(spawn process)。因此 git adapter 全部落在 project,share 不参与;`share::adapter::git` 现仅有 `GitAdapter<T>` newtype,本次不动。

### project(workspace 切片 = 所有者)

- **`WorkspaceState`** `{ initial_cwd: PathBuf, working_root: PathBuf, path_base: PathBuf, stack: Vec<WorkspaceFrame> }` —— 唯一可变 workspace 真相。
- **`WorkspaceFrame`** `{ path_base: PathBuf, working_root: PathBuf }` —— 替代 `share::WorkingContext`,worktree 栈帧。
- **`WorkspaceService`** —— 包 `Arc<Mutex<WorkspaceState>>`,后面**一把锁**,enter/exit 原子切换 root/base/stack(修掉撕裂读)。实现下述三个 trait;并提供 `seed_isolated()`:从当前快照派生独立实例(继承当前 root/base、空栈、新锁),供子 agent。
- **三个 inbound 能力 trait(port,定义在 project,被 tools/runtime 消费)**:
  - `WorkspaceRead` = `current_root()` / `current_path_base()` / `resolve(rel)`
  - `WorkspaceControl` = `set_cwd(path)` / `switch_to(path)` / `enter(path, branch)` / `exit()`
    （`switch_to` 做存在性 + 同源校验后的跳转、不压栈帧，供 `ExitWorktree{path}` 使用——比 `set_cwd` 多一层校验）
  - `WorkspacePersist` = `snapshot()` / `restore(dto)`
  - 注:`set_cwd` 是 bash `cd` 的落点(`bash.rs:158` 现调 `set_working_directory`,持久化到 path_base)。因此 `WorkspaceControl` 的消费者 = **bash + EnterWorktree/ExitWorktree**,不含 session 边界(后者只用 `WorkspacePersist`)。
- **`GitWorktreeOps`** —— outbound port,trait **与默认实现 `GitCli` 均在 project**(project 允许 spawn,`worktree.rs`/`working_paths.rs` 现已直接 spawn);测试注入 `FakeGit`。方法覆盖现有内联 git 调用:`git_common_dir` / `show_toplevel` / `in_worktree` / `worktree_add` / `current_branch`(`current_branch` 供 tools 的 worktree 展示与 runtime 分支查询使用,二者均经 `GitCli` 路由)。`set_cwd` 的根探测(`detect_working_root`,现也内联 git)改走 `show_toplevel`。
- **转换规则改为纯函数**:`enter(&mut WorkspaceState, &dyn GitWorktreeOps, path, branch)` / `exit(&mut WorkspaceState)` / `set_cwd(&mut WorkspaceState, &dyn GitWorktreeOps, path)` 及校验 helper。锁由 `WorkspaceService` 在边界取一次,内部纯逻辑,不再到处穿 `Arc<Mutex>`。
- **`WorkspaceError`** 枚举,集中 workspace 错误,中文用户消息。
- **退役**:`WorktreeWorkingContext`、`ProjectGateway` / `DefaultProjectGateway`(其 path 方法折进 `WorkspaceService` 构造与 `WorkspaceRead`)。

### tools

- `ToolContext` → **`ToolExecutionContext`**:**删除** `working_root` / `path_base` / `context_stack` 三字段,改持有 `Arc<WorkspaceService>`。对外暴露窄访问器:`workspace_read() -> &dyn WorkspaceRead`(所有 tool)、`workspace_control() -> &dyn WorkspaceControl`(仅 bash + worktree 工具,由 guard 约束)。其余字段(`cancel` / `read_files` / `agent_runner` / `session_reminders` / `memory_config` / `plan_mode` / `allow_all` / 并发上限 / `agent_semaphore` / `progress_tx` / `parent_session_id`)保留。
- 路径解析消费者(`file_read`/`file_edit`/`file_write`/`glob_tool`/`grep`/`lsp`/`agent_tool` 等,各处现调 `project::api::current_path(&ctx.path_base)`)改走 `ctx.workspace_read()`。
- **删除** `WorktreeContextExt` 投影(无三字段包可投影);worktree 工具改走 `ctx.workspace_control()`,bash `cd` 改走 `ctx.workspace_control().set_cwd()`。
- `AgentRunRequest.ctx` 类型随之变为 `&ToolExecutionContext`。

### runtime

- **删除** `ToolContextParts` 与 `build_tool_context`(`tool_context.rs`),直接由 `Arc<WorkspaceService>` 构建 `ToolExecutionContext`(`loop_runner.rs:132-147`)。
- **`WorkspaceService` 由 runtime client(`AgentClientImpl`)持有,跨 chat 轮次存活**,取代现有 `inner.workspace_context: Mutex<Option<WorkspaceContext>>`(`trait_session.rs`)以及每轮 loop 的 seed/回写(`loop_runner.rs:98-116`):loop 不再 `new_working_paths` 或从 workspace_context 重建三字段,而是直接用持有的 service。
- session 保存:`service.snapshot()` → `PersistedWorkspaceContext` →(经现有 `mapping::workspace_context_to_sdk`)→ session 存储(`trait_session.rs:37-42`)。
- session 恢复:读 DTO → `service.restore(dto)`(`trait_session.rs:92-100`)。
- `agent_calls.rs:113-115`(task 快照里读 `working_root` + `workspace_context()`)改走 `service.snapshot()` / `workspace_read()`。
- 子 agent:`CliAgentRunner::run_agent`(`setup.rs:165-184`)现在 `Arc::clone` 父 `working_root`/`path_base`(隔离 bug),改为 `parent_service.seed_isolated()` 造子实例;**共享 `agent_semaphore`**,workspace/`read_files`/`session_reminders` 隔离。

### composition

- `wire_project()` 改为返回 `WorkspaceService` 的构造器/provider 并绑定 git adapter;移除 `ProjectGateway` 装配。

## 数据流

- **启动**:runtime client 构造并持有 `Arc<WorkspaceService>`(跨 chat 轮次存活)。
- **工具批次**:runtime 用句柄构建 `ToolExecutionContext`(`workspace_read` 给所有 tool)。
- **EnterWorktree / ExitWorktree**:工具只解析参数 → `ctx.workspace_control().enter(path, branch)` → `WorkspaceService` 取锁一次 → 纯 `enter(&mut state, &dyn GitWorktreeOps, …)`(push 帧、原子换 root/base)→ 返回 `WorkspaceFrame` 或 `WorkspaceError` → 工具映射成 `ToolResult`。
- **bash `cd`**:`ctx.workspace_control().set_cwd(path)` → 取锁一次 → 纯 `set_cwd(&mut state, &dyn GitWorktreeOps, path)`(经 `show_toplevel` 探测 root、换 path_base,不动栈)。
- **session 保存**:`service.snapshot()` → `PersistedWorkspaceContext` →(`mapping::workspace_context_to_sdk`)→ storage 落盘。
- **session 恢复**:读 DTO → `service.restore(dto)` → 全校验后一次性替换。

## 子 agent 隔离

- `parent_service.seed_isolated()`:继承父**当前** root/base,空栈、独立锁。
- **共享**:`agent_semaphore`(全局限流,必须共享)。**隔离**:workspace、`read_files`、`session_reminders`。
- 结果:子 agent enter/exit 自己的 worktree 不触碰父 agent。

## 错误处理

- `WorkspaceError`(project)覆盖:路径不存在 / 不在工作区 / git worktree add 失败 / 仓库不同源 / 栈空 / restore 路径失效 / restore 帧无效 / 锁中毒。中文消息。
- 工具层不拼业务错误,只 `WorkspaceError → ToolResult::error`。
- **session restore 全有或全无**:先校验所有路径与栈帧,任一无效则整体失败,杜绝半恢复态。
- 锁中毒沿用现有 `into_inner()` 韧性恢复。

## 测试策略

- **WorkspaceState 单测(注入 FakeGit,免真实 git)**:init 一致;相对路径基于 path_base;绝对路径原样;`set_cwd` 换 path_base 并经 `show_toplevel` 探测 root;enter push 帧;exit pop 恢复;空栈 exit 报错;snapshot↔restore 对称;restore 无效路径整体失败。
- **worktree 规则单测(FakeGit)**:branch 清洗;同源判断;嵌套拒绝;残栈自愈。
- **GitCli adapter(project)**:保留少量真实 git 集成测试。
- **ToolExecutionContext**:不暴露重复 workspace 字段;tool 经 `WorkspaceRead` 取路径;`plan_mode` / `allow_all` / `progress` / `read_files` / `agent_runner` / `session_reminders` 仍贯通。
- **session**:保存含 `PersistedWorkspaceContext`;恢复后一致;旧 JSON 字段兼容。
- **子 agent 隔离**:父进 worktree → 派子 → 子由快照 seed → 不共享父锁 → 子 enter/exit 不影响父 → semaphore 仍共享。
- **回归**:`cargo test -p runtime`、`cargo test -p tools`、`cargo test -p project`、`cargo check`。

## 架构 Guard(接入 `.agents/hooks/check-architecture-guards.sh`)

规则落在独立守卫 `.agents/hooks/check-context-architecture.sh`(R1–R6),由 `check-architecture-guards.sh` 串联调用:

- **R1** `ToolExecutionContext` 不得含 `working_root` / `path_base` / `context_stack` 字段。
- **R2** tools 不得直接引用 `PersistedWorkspaceContext` 或 `WorkspacePersist`(DTO / 持久化只在 session 边界)。
- **R3** 仅 project 可定义 `WorkspaceState`;`agent/features` 内(project 除外)任何 struct 不得同时打包 `working_root` + `path_base` + (`context_stack`|`stack`)(防 `WorktreeWorkingContext` 复活)。**narrowing**:triple-bundle 检测不扫 `agent/shared`(持久化 DTO `PersistedWorkspaceContext`)与 `packages/sdk`(`WorkspaceContextView` 视图),二者是设计允许的序列化/投影形态,而非运行期可变三元组。
- **R4** 生产代码调 `.workspace_control()` 仅限 tools 的 `business/bash.rs` 与 `business/worktree.rs`。
- **R5** git 仅经 `GitWorktreeOps`;**在 `agent/features/project/` 范围内**,`Command::new("git")` 仅可出现在 `business/git_ops.rs`(`GitCli` adapter)。该规则 **SCOPED 到 project**,不做全仓库 git 禁令——runtime 另有与本重构无关的生产 git spawn(`business/prompt/build/git_context.rs` 的 prompt git 上下文、`core/command/commands/git.rs` 的 `/git` slash 命令),不在本重构范围内。
- **R6** `WorkspacePersist` 仅可出现在 project(def/impl)与 **runtime(广泛允许,不限于 `core/client/`——`business/chat/looping/{agent_calls,non_agent,post_batch}.rs` 等亦调 `snapshot`)**;tools 禁用(与 R2 重叠)。
- **替换** `check-crate-api-boundary.sh` 中针对 `WorktreeContextExt` 的旧豁免(`TOOLS_PROJECT_CONTEXT_API_NAMES` 等):投影删除后该豁免失效,已删除,改为本节 R1–R6 新规则。
- 触发范围:diff 命中 `agent/features/runtime/**`、`agent/features/tools/**`、`agent/features/project/**`、`agent/shared/src/session_types.rs`、`agent/shared/src/tool.rs`。
- 原则:CI / 本地 / Stop hook 复用同一检查入口;hook 只调用,不内嵌规则;失败消息须说明违反的约束与修复方向。

## 分阶段迁移

1. **share**:`WorkspaceContext` → `PersistedWorkspaceContext`(serde 兼容别名);`WorkingContext` 移出(短期别名)。
2. **project**:落 `WorkspaceState` / `WorkspaceFrame` / 三 trait / `WorkspaceService` / `GitWorktreeOps` + `GitCli`;`enter`/`exit`/`set_cwd` 重写为纯规则(`set_cwd` 收编 `working_paths::set_working_directory` + `detect_working_root`);旧函数留临时 shim。
3. **tools**:三字段 → workspace 句柄;`ToolContext` → `ToolExecutionContext`;路径工具改走 `workspace_read()`;删 `WorktreeContextExt`;worktree 工具改走 `workspace_control()`,bash `cd` 改走 `set_cwd()`。
4. **runtime**:删 `ToolContextParts`/`build_tool_context`;client 持有 `WorkspaceService` 跨轮存活,移除 per-loop seed(`loop_runner.rs:98-116`)与 `inner.workspace_context`;`agent_calls.rs:113-115` 改走 `snapshot()`/`workspace_read()`;`trait_session.rs` 接 snapshot/restore;`setup.rs` 子 agent 改 `seed_isolated()`。
5. **composition**:`wire_project` 返回 service provider(+ 注入 `GitCli`);移除 `ProjectGateway`/`DefaultProjectGateway` 及 `composition/src/project.rs`、`app.rs` 中相关装配。
6. **收尾**:删 shim/别名;改/加 guard(替换 `WorktreeContextExt` 豁免);补测试;`cargo check` + clippy。

## 非目标

- 不改 session JSON 字段语义、不破坏旧 session 兼容。
- 不重做 permission / policy / hook 体系。
- 不改 tool schema 或 TUI 展示协议。
- 不引入跨进程 workspace 状态共享。

## 验收标准

- 运行时 workspace 可变状态只有 `WorkspaceState` 一个 owner(在 project)。
- `ToolExecutionContext` 不再平铺 workspace 三字段。
- tools/runtime 经三个能力 trait 访问 workspace;持久化 DTO 与运行态边界清晰。
- worktree 切换原子化(单锁,无撕裂读)。
- 子 agent 不共享父 agent workspace 锁;子 agent worktree 切换不影响父 agent;`agent_semaphore` 仍共享。
- git 经 `GitWorktreeOps`,`GitCli` 外无内联 git;WorkspaceState 可注入 FakeGit 做纯单测。
- 架构 guard 接入 `.agents/hooks/check-architecture-guards.sh`,Stop hook 可阻止违反 context 架构规则的变更。
- 相关单元测试、架构测试与 `cargo check` 通过。
