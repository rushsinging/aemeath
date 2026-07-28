# Runtime 引擎

**Scope**：`agent/features/runtime/**`——Agent 主循环、tool 执行编排、token budget、对话压缩（compact）、成本追踪、slash 命令系统。
**主触发**：改 `agent/features/runtime/**`。
**次触发**：改暂停 / 恢复 / 重试逻辑；改成本追踪；新增 slash 命令。
**配套**：Tool Published Language、Catalog/Execution 端口与 MCP 主体在 `tools.md`；provider 调用在 `provider.md`。

## 统一运行链路决策（#1397）

- Main Run 与派生 Run **MUST** 使用同一准备、启动与 Loop Engine 链路。
- Runtime **NEVER** 按 Main/Sub 角色恢复第二套模型调用、工具编排、交互、Step 收口或 Stop Hook 流程。
- Run 来源差异 **MUST** 通过窄能力契约表达；**NEVER** 聚合为角色化 fat port。
- `Run` **MUST** 独占领域生命周期状态，`RunExecutionState` **MUST** 独占执行期工作状态，`RuntimeContext` **MUST** 只持本次 Run 已绑定能力；三者 **NEVER** 复制同一事实。
- Runtime Context 的生产构造 **MUST** 只有一个入口；调用方 **MUST** 只提交纯值 Run 请求，**NEVER** 手填能力绑定或同构参数包。
- 模型调用、工具轮次、Interaction、Step transaction 与 Stop Hook **MUST** 分别只有一个 application owner。
- Runtime application **NEVER** 恢复 Main/Sub 角色目录、平铺兼容转发或无明确所有权的 `common` / `shared` 容器。
- Runtime 生产代码 **NEVER** 使用模块级或宽泛 item 级 `allow(dead_code)`；测试专属能力 **MUST** 受 `cfg(test)` 或 test-only feature 约束。
- Runtime 类型、trait、模块、函数、方法与变量 **NEVER** 使用 `Projection` / `projection` 宽泛命名；职责混合的抽象 **MUST** 先拆分，数据转换 **MUST** 按来源、目标或用途命名。
- 可由依赖方向机械表达的边界 **SHOULD** 使用禁止 import 规则；唯一实现、状态唯一所有权和目录所有权 **MUST** 使用结构守卫验证。

## 配置应用边界（#1345）

- Runtime **MUST** 在接纳真实用户输入、创建 Main Run 前调用 `ConfigReader::refresh_if_sources_changed()`；同一次 Run 内 **NEVER** 再 refresh。
- Main Run 与每个新 Subagent Run **MUST** 捕获一个 `RunConfigSnapshot`；provider binding、WebSearch 的 HTTP `User-Agent`、`allow_all`、hooks、language、timeout 与 context size 随该 Run 固定。
- `api.user_agent` 属于 `Run` scope：Provider 与 WebSearch 在下一 Run 使用冻结快照；当前 Run 不变。
- Run Step 只从所属 `RunConfigSnapshot` 构造 `ContextRequest.config_snapshot`，**NEVER** 读取 `ConfigReader` / `ConfigQuery` 或配置文件。
- `SessionRestartRequired` scope 仅标记 pending session revision；当前 TUI/logger/MCP/storage 基础设施保持不变，成功 session resume 后清除 pending 标记。

## 3.9.1. 会话历史唯一真相（#872）

- **MUST** 会话历史唯一可变真相属于 Context Management 的 `CanonicalSession` backing。
- **MUST** Runtime 每个 Run 用显式 Step message ownership 记录当前 Run/RunStep 的消息投影；**NEVER** 通过消息位置、长度、历史数量或索引推断归属。历史通过 `ContextPort::build_window` 读取，finalized Step 通过 `append_and_persist` 提交。
- **NEVER** Runtime 生产代码引用 `context::session::*`、`ChatChain` / `ChatSegment`，或恢复 `current_chain` / `frozen_chats` / `active_summary` 第二 backing。
- **NEVER** 恢复 `save_chain`、loop-exit auto-save 或 Runtime 自写 Session 文件；持久化由 Context 复用 Storage AtomicBlob。
- **MUST** `ChatRequest` 只传增量 `user_input`，**NEVER** 传全量消息历史。
- **MUST** idle `/compact`、reset 与自动 compact 经 ContextPort；启动 resume 与运行期 `/resume` 经同一 `MainSessionWiring::resume_session` 协调器。

## 3.9.2. Tool 执行编排

- 执行流程：LLM 返回 tool_use → Runtime 取得本次 Scope/Profile 的 Catalog snapshot → Policy/Hook/并发/timeout 编排 → 经 `ToolExecutionPort` 执行 → 结果注入回消息。
- #911 已完成生产双端口切线：Main/Sub Runtime 只持 `Arc<dyn ToolCatalogPort>` / `Arc<dyn ToolExecutionPort>` 与 Published Language，不持 `ToolRegistry`、不取得或调用 `Tool` 实例。生产装配入口在 `application/client/from_args.rs`，Tools 私有 backing 与双 adapter factory 见 `tools.md`。
- Catalog 提供 schema/并发/timeout 描述；Execution 复验存在性、Scope/Profile、schema 后调用 Tool。schema 所有权归 Tools，Runtime 的 `application/agent/input_validation.rs` 仅为兼容 re-export / phase peel。
- Runtime 自行持有 `WorkspacePersist`、并发 semaphore、timeout、Policy/Hook、取消实现与 interaction waiter；这些 **NEVER** 流入 Tools domain。`WorkspaceViews` 只在 `application/tool_execution_adapters.rs` 转成窄 live capabilities，`ExecutionScope` 只传纯值快照。
- Tools 返回 typed `ToolSuspension`；Runtime 在 `application/suspension_mapping.rs` 映射为现有 AskUser 交互值并拥有等待。#911 只完成 suspension 边界和映射 seam；#877/#878 的完整 Interaction identity、continuation、`AwaitingUser` / resume / cancel 状态机仍未完成，旧 Runtime-owned AskUser oneshot 仍是兼容生产路径。
- #912 已完成 Skill ownership 与 Main/Sub 装配：正式 Tool Catalog/Execution 不含 Skill；Main/Sub Context 都经 Skill-owned materialization 和 live WorkspaceRead snapshot 注入 PromptFragment。#913 的其余 Runtime/Composition ownership 收口仍未完成；#914 负责旧内部 Registry/Profile/legacy-no-agent/SkillTool 文件的最终物理退役。MCP Ready 生命周期与 Catalog revision 也不属于 #912。

## 3.9.3. token budget / 压缩 / 成本

- token 估算由 Context BC 的 `context::api::compact::estimate_tokens` 提供，Runtime 在 `application/{agent,chat}` 编排中消费。
- **SHOULD** 修改涉及暂停 / 恢复 / 重试逻辑时同步检查 Context token estimation 调用点。
- 成本追踪与定价：`agent/features/runtime/src/application/cost/pricing.rs`。
- **SHOULD** 成本追踪逻辑更新时同步更新 `pricing.rs`。
- 成本历史落盘在 `~/.agents/cost_history.json`。

## 3.9.4. slash 命令系统

- slash 命令的 SDK/TUI 入站路由属于 `application/client`；具体命令能力由对应 Feature 的 Published Language / Tool 提供。
- #913 已完成 Command Catalog/Router 生产切线：Command PL 与双端口由 Tools 独占，SDK 直接 re-export，Composition 向 TUI/no-TUI 注入同一 Catalog/Router；Runtime `PendingCommand` 仅作为目标 BC handler adapter，**NEVER** 再解析 slash 名称、alias 或参数 schema。
- Runtime 内不再维护 `core/command` 固定层注册表；新增命令必须先进入 Tools-owned Descriptor/Catalog，并由实际 owner 提供 handler，**NEVER** 恢复 SDK/TUI/Runtime 静态清单。
- 命令在 TUI 的展示样式见 `tui-cli.md`。
