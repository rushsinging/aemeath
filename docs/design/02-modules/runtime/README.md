# Agent Runtime（核心域）

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> Agent Runtime 是唯一核心域 BC：驱动"推理 → 工具 → 观察"循环、维护单一 Run 状态机、编排模型调用与工具执行、派生与执行 SubAgent。

## 三元组速览

| 概念 | 回答 | 性质 |
|---|---|---|
| **RunSpec** | 跑什么（prompt/tools/model/timeout/资源模式）| 声明式、可序列化 |
| **RuntimeContext** | 用什么资源跑（供应能力 OHS + Runtime-owned detail ports + config）| 装配的活资源 |
| **Run** | 一次执行实例 | 内存态聚合 + 唯一 Agent 执行生命周期状态机 |

因果链：`RunSpec ──装配──▶ RuntimeContext ──注入──▶ Run`；层级 `Session → Run → Run Step`。

## 核心设计约束

1. **单执行生命周期状态机**：全系统只有 `Run` 驱动 Agent 执行生命周期（内存态、不持久化、崩溃从头开始）；其他 BC 可拥有不驱动 Run 的局部状态机
2. **Loop Engine 零来源分支**：所有 Run 共用一套 Loop；来源差异 100% 在 RunSpec + RuntimeContext capability adapter，NEVER 形成 Main/Sub 生产类型
3. **IoC 装配**：Runtime 定义 factory/port contracts 与能力选择规则，Composition 只实现和连接；调用方只提交 `RunCreationRequest`
4. **单能力直接分层**：仓库 `agent/features/*` 是 VSA；事实核验后 Runtime 当前只有一个完整业务能力 `agent_execution`，因此不增加单元素 `capabilities/agent_execution` 包装，直接在 crate 根按 `domain/application/ports/adapters` 组织
5. **内部角色不是平级 slice**：`agent_run` 是领域模型，Loop Engine 与各 coordinator 是应用编排，事件投影是 adapter；未来出现多个具有独立用例、状态所有权和变化轴的真实能力时才递归竖切
6. **唯一生产装配**：具体实现实例化和对象图连接全部位于 `agent/composition`；能力决策与统一 `RuntimeContextFactory` 契约属于 Runtime，Runtime 内不建立第二个 Composition Root
7. **Activity 观测单一机制**：`ActivityCoordinator` 统一创建 / 变更 Runtime Activity 事实；Activity 不构成新的 BC、聚合根或 Run 状态机。SDK 发布增量与快照，TUI 只通过 ACL / reducer 建立事实镜像，再派生低噪声摘要
8. **安全铁律**：派生 Run 能力 ≤ 父 Run（只削弱不越权）
9. **防 stuck 内置**：StuckGuard 四层防线对所有 Run 统一保护
10. **Session 回放边界**：Session committed content 是唯一可回放数据源；Runtime 内存态与未入 Session 的 InputBuffer 不可回放，Terminate 时可丢弃
11. **唯一 accepted input**：Session gate 将 `UserMessage` / `SkillRequest` 收敛为 `AcceptedUserInput`；admission、buffer、drain、freeze、Context append 与 adoption 只沿这一条 typed 数据流推进，NEVER 并行维护 event/message adopted payload，也 NEVER 从模型正文反向解析输入类型
## 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | Run 聚合、RunSpec、RuntimeContext、实体/VO、不变量、控制权矩阵、安全铁律、差异矩阵 |
| [02-module-boundaries.md](02-module-boundaries.md) | 单一 `agent_execution` 能力的六边形分层、内部角色、状态所有权与依赖方向 |
| [03-loop-and-state-machine.md](03-loop-and-state-machine.md) | Run 单状态机、Loop Engine 零分支骨架、单 Run vs Session 多 Run 序列 |
| [04-stuck-prevention.md](04-stuck-prevention.md) | StuckGuard 四层防线、分级响应、状态机集成 |
| [05-recovery-semantics.md](05-recovery-semantics.md) | 从头开始恢复、持久化边界、无 durable |
| [06-ports-and-adapters.md](06-ports-and-adapters.md) | 入站 OHS、Runtime 消费的能力契约、RuntimeContext 装配、Composition Root |
| [07-runtime-ownership-and-assembly.md](07-runtime-ownership-and-assembly.md) | Runtime IoC 终态：去除 Main/Sub 生产类型、静动态所有权、统一 prepare/factory、Interaction/Hook capability adapter 与 Composition 边界 |
| [08-event-pipeline-and-published-language.md](08-event-pipeline-and-published-language.md) | Runtime 事件体系稳定入口：事实 owner、authority、identity、ordering、delivery、Published Language 与变更门禁 |
| [events/README.md](events/README.md) | 编号事件分片导航；包含统一命名规范、事件族、Lifecycle、Activity、Published State、Content Stream、Interaction/Control、Catalog/Config 与全量索引 |

## 相关文档

- Session 聚合（属 Context Management）：[../context-management/01-session.md](../context-management/01-session.md)
- 系统级总体设计：[../../01-system/](../../01-system/)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：模块入口 + 三元组速览 + 文档导航 | #761 |
| 2026-07-12 | 出站端口数量改为开放表述，适配 Tool Catalog/Execution 拆分 | #787 |
| 2026-07-15 | 经源码与用例边界复核，Runtime 当前只有一个 `agent_execution` 能力；撤销八模块平级竖切，改为 crate 根直接采用轻量六边形 | [#995](https://github.com/rushsinging/aemeath/issues/995) |
| 2026-07-15 | 曾将 Runtime 内部角色误判为多个平级能力并递归竖切；此结论已由上一条复核记录取代 | [#995](https://github.com/rushsinging/aemeath/issues/995) |
| 2026-07-15 | 明确 CancelRunStep/TerminateRun 共用 deterministic StepFinalizer；Session committed content 是唯一回放源，未入 Session 的 InputBuffer 在 Terminate 时可丢弃 | [#700](https://github.com/rushsinging/aemeath/issues/700) |
| 2026-08-01 | 补充统一 Activity 观测机制与 Runtime / SDK / TUI 边界 | 统一 Activity 观测 |
| 2026-08-08 | 将 Runtime 事件目录按事实 owner 与事件族拆为编号分片，新增统一命名规范、全量跨层索引和历史名称迁移清单 | 事件体系治理 |
