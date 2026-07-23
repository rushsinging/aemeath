# 02-modules · 模块级设计

> 层级：02-modules（模块 / BC 战术设计）
> 状态：Target｜Milestone：v0.1.0｜对应 Issue：#761 / [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本层承载各 Bounded Context 的**战术设计**：聚合、实体、值对象、不变量、领域服务、模块内端口与内部结构。**只描述目标态。** 总体战略设计见 [../01-system/](../01-system/)。

## 模块文档

每个 BC / 模块一份文档，用数字前缀命名：

| 目标文档 | 内容 |
|---|---|
| [runtime/](runtime/README.md) | Run 聚合、单状态机、Loop Engine、防 stuck、恢复语义、端口与装配 |
| [context-management/](context-management/README.md) | Session 聚合、Compact 家族（五级管线）、Token Budget、Prompt / Guidance、Memory 注入 |
| [tools/](tools/README.md) | Tool Catalog / Execution 双端口、Scope / Profile、Skill、Slash Command 与 MCP 生命周期 |
| [task/](task/README.md) | TaskStoreState 聚合根、Task 局部生命周期、依赖图不变量、Batch lifecycle、TaskAccess / TaskPersist、Published Language |
| [project/](project/README.md) | Workspace 聚合根、Frame 栈、fork 隔离、三端口、GitWorktreeOps、git 上下文供给 |
| [memory/](memory/README.md) | MemoryEntry 聚合、检索与注入、Reflection 引擎、MemoryPort |
| [provider/](provider/README.md) | Provider ACL、统一调用流、模型能力、reasoning 映射与不可变 Invocation Scope |
| [workflow/](workflow/README.md) | ReasoningNode 状态机、effort 调节、ReasoningPort OHS 与 clamp 不变量 |
| [config/](config/README.md) | Config 分层优先级链、ConfigSnapshot PL、Config-owned OHS / project participant、CompatibilityAdapter ACL |
| [tui/](tui/README.md) | 八层 TEA 管线、六 Context 投影、Intent / Change / Effect、SDK ACL、ViewAssembler / ViewModel / Render 与四类 Interaction 资源隔离 |
| [storage/](storage/README.md) | 原子读写、backup / quarantine、路径安全及数据所有权边界 |
| [logging/](logging/README.md) | 14 字段诊断 schema、TargetCatalog、scope-local context、sink / rotation 与 Audit 分离 |
| [application-version-control/](application-version-control/README.md) | typed channel、检查缓存、Release ACL、VerifiedUpdatePlan 与安装事务 |
| [policy/](policy/README.md) | AllowAll-only Policy 实现范围与三态 PolicyPort 扩展边界 |
| [hook/](hook/README.md) | 单 HookPort、类型化协议、3 次执行重试与 Stop / Run 15 次阻断语义 |
| [audit/](audit/README.md) | Usage-only Audit MVP、非阻塞 Sink / Query 与独立 JSONL 存储 |
| [server/](server/README.md) | WS 协议、控制面 / worker 拓扑的 Future 设计边界 |

## 目录结构决策

系统级判据以[代码组织规范](../01-system/06-code-organization.md)为唯一真相源。所有 feature crate 内部统一采用 Hexagonal 依赖方向（`domain ← application ← ports ← adapters`）；小模块 MAY 只使用部分层（如 `domain + adapters`），**NEVER** 为对称预建空层。`capabilities/` 降格为仅在 §3.1 证据成立时的可选竖切结构。

| 模块 | Target 结构 | 判定原因 |
|---|---|---|
| Agent Runtime | `domain/application/ports/adapters` 四层 | 核心执行模块的战术设计已冻结该 Hexagonal 物理结构 |
| Context Management | `domain/application/ports/adapters` 四层 | Session、Compact、Token Budget、Prompt/Guidance、Memory Injection 按层共置 |
| Memory | `domain/application/ports/adapters` 四层 | write、retrieve、compact、reflection 策略收在 domain，用例编排收在 application |
| Tool & Skill & Command | `domain/application/ports/adapters` 四层 | catalog、execution、skill、command 策略在 domain；MCP 技术 detail 在 adapters |
| Storage | `domain + ports + adapters` 三层 | `safe_path`、`atomic_blob`、`atomic_dataset` 的 Published Language 与策略在 domain，OHS 在 ports，文件系统 detail 在 adapters；稳定层名和单向依赖可机械阻止路径/I/O 下沉、adapter 泄漏与 façade 漂移 |
| Task Management | `domain + adapters` 两层 | transition、dependency、batch、snapshot 共同守护同一 `TaskStoreState` 聚合 |
| Project / Workspace | `domain + adapters` 两层 | Workspace 是唯一聚合与状态源；Git 仅是可替换外部 detail |
| Workflow | `domain + adapters` 两层 | v0.1.0 仅有 Reasoning Graph / effort 调节，无第二个独立变化轴 |
| Policy | `domain + adapters` 两层 | v0.1.0 只有 Policy evaluate 与 AllowAll 实现，无内部子能力或出站 seam |
| Provider | `domain + ports + adapters` 三层 | invoke/capability/error 策略在 domain；Anthropic/OpenAI-compatible/Ollama 技术实现在 adapters |
| Config | `domain + adapters` 两层 | 只有一条 effective-config 生命周期；File/Env/CLI/Compatibility 是不同外部来源 |
| Hook | `domain + ports + adapters` 三层 | 单一 Hook dispatch 能力在 domain；进程执行与类型化协议在 adapters |
| Audit | `domain + adapters` 两层 | v0.1.0 只拥有 Usage；ingest、append 与 query 是同一 schema 的处理管线 |
| Logging | `domain + adapters` 两层 | 各目录是同一诊断记录流水线阶段，不具备独立业务状态所有权 |
| Application Version Control | `domain + ports + adapters` 三层 | check/plan/apply 共享 `VerifiedUpdatePlan`；Release Source 和 installer 是外部 seam |
| TUI（交付层） | adapter/model/update/effect/view/render TEA 技术目录 | 目录承载单向数据流和 import 隔离，不是 Bounded Context 内的业务竖切 |
| Server（Future） | 暂不冻结 | 安全、部署和传输边界尚未完成正式设计，**NEVER** 由当前 sketch 预建目录 |

采用 `capabilities/` 的模块 **MUST** 使用私有 `capabilities.rs` + `capabilities/<slice>.rs` 形状；其他模块 **NEVER** 为对称性创建该目录。局部 model、port、adapter 仍按真实不变量和 seam 独立判定。

## 编写原则

- 只描述目标态，区分 Target / Decision，不记录当前代码状态。
- 每篇独立成文，带"相关文档"链接与"修改历史"。

## 相关文档

- 系统级总体设计：[../01-system/](../01-system/)
- 工程守则：[../03-engineering/README.md](../03-engineering/README.md)
- Current → Target 迁移治理：[../03-engineering/03-migration-governance.md](../03-engineering/03-migration-governance.md)
- 目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：承接说明 + 规划模块清单 | #760 |
| 2026-07-11 | 改为纯目标态（移除"承接现有文档"迁移列）、链接化、新增修改历史 | #760 |
| 2026-07-11 | 术语改名：Agent Execution→Agent Runtime、AgentRun→Run | #760 |
| 2026-07-11 | S2 填充 runtime/（7 篇）与 context-management/session.md，规划表改链接 | #761 |
| 2026-07-12 | 新增 tools/ 战术设计：Tool 双端口、Scope/Profile、Skill/Command 与 MCP 生命周期 | #787 |
| 2026-07-12 | 新增 provider/ 战术设计：ProviderPort、ACL、流语义、模型能力与 Invocation Scope | #788 |
| 2026-07-12 | 新增 context-management/ 02-05：Compact 家族、Token Budget、Prompt/Guidance、Memory 注入 | #786 |
| 2026-07-12 | 新增 workflow/ 与 config/ 战术设计：ReasoningNode、ReasoningPort、Config 分层、ConfigSnapshot PL | #792 |
| 2026-07-12 | 新增 Storage、Logging、Application Version Control 三个通用域摘要设计 | #793 |
| 2026-07-12 | 新增 memory/ 战术设计：MemoryEntry 聚合、检索与注入、Reflection 引擎、MemoryPort | #789 |
| 2026-07-12 | 新增 task/ 战术设计：Task 聚合、状态机、依赖图不变量、Batch、TaskPort、PL | #791 |
| 2026-07-12 | 新增 project/ 战术设计：Workspace 聚合、Frame 栈、fork、三端口、git 供给 | #791 |
| 2026-07-12 | 新增 server/ 占位文档：暂缓设计，继承草案约束 | #794 |
| 2026-07-12 | 新增 tui/ 战术设计：八层 TEA 管线、三条信息流、3+1 Context、SDK DTO 边界、架构门禁、死代码清单、reducer 纯化目标态 | #795 |
| 2026-07-12 | 新增 tui/02-model：3+3 Context 完整字段、投影状态机、SpinnerPhase 派生函数、RunRuntimeState 6 子模块、ConfigProjection、WorkspaceProjection、单一真相规则、Model 纯净性约束 | #796 |
| 2026-07-12 | 新增 tui/03-event-flow-and-acl：事件流两层转换 ACL、SDK DTO 边界、agent_id 缺口 R8、sub-agent 事件路由 #612、转换集中化、架构门禁 #6 | #797 |
| 2026-07-12 | 新增 policy/hook/audit 战术设计：AllowAll-only、Hook 3/15 两层重试、Usage-only Audit MVP | #790 |
| 2026-07-16 | 增加模块 Target 目录结构决策矩阵：按 Hexagonal 默认依赖方向选择最小必要层，`capabilities/` 只在有独立能力证据和配套 Guard 时启用 | [#972](https://github.com/rushsinging/aemeath/issues/972) / [#991](https://github.com/rushsinging/aemeath/issues/991) |
| 2026-07-16 | 将 Storage 冻结为 `domain + ports + adapters`：以稳定层名和单向依赖降低 Guard 成本，机械防止物理 I/O 下沉、adapter 类型泄漏与 façade 漂移 | [#880](https://github.com/rushsinging/aemeath/issues/880) |
