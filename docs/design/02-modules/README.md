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

系统级判据以[代码组织规范](../01-system/06-code-organization.md)为唯一真相源。本表只冻结各模块应用判据后的 Target 形状；`capabilities/` 表示已证明的业务竖切，技术目录表示同一能力内部的外部协议或处理管线，二者 **NEVER** 混用。

| 模块 | Target 结构 | 判定原因 |
|---|---|---|
| Agent Runtime | 保持 `domain/application/ports/adapters` | 核心执行模块的战术设计已冻结该 Hexagonal 物理结构；这是显式例外，**NEVER** 推广为其他模块模板 |
| Context Management | `capabilities/` 竖切 | Session、Compact、Token Budget、Prompt/Guidance、Memory Injection 拥有独立词汇、变化原因与测试夹具 |
| Memory | `capabilities/` 竖切 + 必要共享 model | write、retrieve、compact、reflection 规则可独立变化；`MemoryEntry` 不变量由共享 model 唯一维护 |
| Tool & Skill & Command | `capabilities/` 竖切 | catalog、execution、skill、command、MCP 的契约、消费者和生命周期不同；MCP 技术 detail 留在所属切片 |
| Storage | `capabilities/` 竖切 + 私有 filesystem 技术 detail | `safe_path`、`atomic_blob`、`atomic_dataset` 拥有不同协议与故障测试；文件 driver 是私有 seam，不形成横向 adapter 层 |
| Task Management | 单能力扁平 | transition、dependency、batch、snapshot 共同守护同一 `TaskStoreState` 聚合，无独立能力所有权 |
| Project / Workspace | 单能力扁平 + `git` 局部技术 seam | Workspace 是唯一聚合与状态源；Git 仅是可替换外部 detail |
| Workflow | 单能力扁平 | v0.1.0 仅有 Reasoning Graph / effort 调节，无第二个独立变化轴 |
| Policy | 单能力扁平 | v0.1.0 只有 Policy evaluate 与 AllowAll 实现，无内部子能力或出站 seam |
| Provider | 扁平核心 + provider/protocol 技术目录 | invoke/capability/error 共同形成统一 ACL；Anthropic/OpenAI-compatible/Ollama 才是技术变化来源 |
| Config | 扁平核心 +来源 adapter 技术目录 | 只有一条 effective-config 生命周期；File/Env/CLI/Compatibility 是不同外部来源，不是业务切片 |
| Hook | 扁平核心 + executor/protocol 技术目录 | 单一 Hook dispatch 能力共享匹配、重试和 directive 语义；进程执行与安全边界属于技术 detail |
| Audit | 扁平 Usage 能力 + append/query 技术实现 | v0.1.0 只拥有 Usage；ingest、append 与 query 是同一 schema 的处理管线，Cost/Pricing 仍是 Future |
| Logging | schema/filter/routing/sink/lifecycle 技术管线 | 各目录是同一诊断记录流水线阶段，不具备独立业务状态所有权 |
| Application Version Control | 扁平 update 能力 + source/installer 技术目录 | check/plan/apply 共享 `VerifiedUpdatePlan` 与同一安装事务；Release Source 和 installer 才是外部 seam |
| TUI（交付层） | adapter/model/update/effect/view/render TEA 技术目录 | 目录承载单向数据流和 import 隔离，不是 Bounded Context 内的业务竖切 |
| Server（Future） | 暂不冻结 | 安全、部署和传输边界尚未完成正式设计，**NEVER** 由当前 sketch 预建目录 |

采用 `capabilities/` 的模块 **MUST** 使用私有 `capabilities.rs` + `capabilities/<slice>.rs` 形状；其他模块 **NEVER** 为对称性创建该目录。局部 model、port、adapter 仍按真实不变量和 seam 独立判定。

## 编写原则

- 只描述目标态，区分 Target / Decision，不记录当前代码状态。
- 每篇独立成文，带"相关文档"链接与"修改历史"。

## 相关文档

- 系统级总体设计：[../01-system/](../01-system/)
- 横切工程：[../03-engineering/README.md](../03-engineering/README.md)
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
| 2026-07-16 | 增加模块 Target 目录结构决策矩阵：竖切统一进入 `capabilities/`，Runtime 保留已冻结 Hexagonal 结构，其余模块按能力证据选择扁平、竖切或技术目录 | [#972](https://github.com/rushsinging/aemeath/issues/972) / [#991](https://github.com/rushsinging/aemeath/issues/991) |
