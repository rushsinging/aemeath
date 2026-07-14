# 依赖规则与铁律

> 层级：01-system（系统级总体设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：[#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义强制性的依赖方向规则。**只描述目标态规则，不记录当前代码状态或迁移进度。** 违反即架构腐化；每条规则的机械可执行性要求与覆盖状态职责见 §6。

## 1. 依赖方向总则（Clean）

> **`external detail -> capability policy` 是源码依赖语义：易变外部细节依赖稳定能力策略，反向依赖 NEVER 出现。**

```text
外部 detail ──依赖 / 实现消费方 port──▶ capability policy

CLI / TUI / WS ──依赖 AgentClient────────────▶ Agent Runtime
HTTP / SSE      ──实现 Runtime 消费方拥有的模型调用 port──▶ Model Invocation 策略
Git CLI / 子进程 detail ──实现 Project 消费方拥有的 git worktree port──▶ Project Workspace 策略
```

箭头表示源码依赖，不表示运行时调用方向。`capability policy` 包含能力拥有的不变量、用例决策、公开 façade 与 Published Language；它是语义边界，**NEVER** 要求对应某个固定目录。`external detail` 包含 UI、协议、框架、网络、文件系统、进程、时钟及其他易变技术实现。

模型调用与工作区交互的具体战术端口名称和职责，分别 **MUST** 以 [Provider 模块设计](../02-modules/provider/README.md) 与 [Project 端口与适配器](../02-modules/project/02-ports-and-adapters.md) 为真相源；本图 **NEVER** 另行命名战术 port。

- 稳定能力策略 **NEVER** import、构造或泄漏易变外部 detail 类型；外部 detail **MUST** 在边界完成 wire type、错误与事件转换。
- 当确认稳定策略若不抽象就会依赖易变 detail 时，该策略 **MUST** 在真实易变边界定义并拥有目的性**出站 port**；port **SHOULD** 靠近消费外部交互的用例。供其他能力调用的**入站 façade / OHS** **MUST** 由供应能力拥有。
- 尚未形成策略 / detail 边界时，能力 **MAY** 保持私有具体依赖，但该依赖 **NEVER** 越过能力 façade，也 **NEVER** 成为跨能力契约。
- 具体生产实现 **MUST** 只由 Composition Root 选择并绑定。为隐藏模块私有 detail，Composition **MAY** 调用能力发布的 opaque production factory；factory **MUST** 只完成模块内部构造，**NEVER** 自行读取全局配置、在候选实现间决策或成为第二个业务装配入口。

## 2. 依赖铁律（强制）

| # | 铁律 | 违反模式 |
|---|---|---|
| R1 | **能力策略 NEVER 依赖外部 detail**：稳定策略 NEVER import、构造或公开易变技术实现类型 | 用例策略 import 某个 provider driver 的具体类型 |
| R2 | **隔离真实易变 detail 的出站 port MUST 由消费策略定义**：port 按交互目的命名，外部 detail 依赖并实现它；供其他能力调用的入站 façade / OHS 由供应能力拥有；没有真实 seam 时 NEVER 预建 | HTTP driver 发布核心调用 trait，或消费方重新包装 Project-owned OHS |
| R3 | **跨能力 MUST 只经窄 façade 或 Published Language 通信**，NEVER 直接 import 对方内部类型 | 一个能力直接 import 另一个能力的内部存储结构 |
| R4 | **外部驱动 detail NEVER 触碰能力内部**：TUI / CLI 只经 `AgentClient`，NEVER import Runtime 内部类型 | UI 里出现 Runtime 的内部上下文类型 |
| R5 | **Config 单向下发**：所有 BC 顺从消费只读 `ConfigSnapshot`，NEVER 反向依赖，NEVER 绕过快照读裸配置 / env 散点 | 业务代码里直接读环境变量 |
| R6 | **Composition Root MUST 是唯一生产装配入口**：具体实现选择、factory 调用与跨能力接线只由组合根发起；模块可在 composition-only opaque factory 内构造私有 detail，但 NEVER 自行选择候选实现或从业务路径触发生产装配 | 能力在业务路径直接 `new` 存储实现，或根据全局配置自行挑选 adapter |
| R7 | **边界模型 MUST 按 Context Map 转换或共享**：provider wire type **MUST** 经 Provider ACL 转为稳定 `Message`；`DomainEvent` **MUST** 经 TUI ACL 转为 TUI Model；只有 [Context Map](03-context-map.md) 登记的 Shared Kernel 类型 **MAY** 在其列明的参与 BC 间跨界（`Message` 用于 Runtime / Context Management / Provider，`ReasoningLevel` 用于 Workflow / Config / Runtime / Context Management / Provider），其他内部类型 **NEVER** 直通 | provider wire type 直接进入 Runtime，或 `DomainEvent` 直接进入 TUI Model，或未登记类型被多个 BC 各自复制 |

## 3. 目录名称不证明依赖方向

目录只用于导航。能力是否守住边界 **MUST** 由源码依赖图、Rust 可见性、受控 re-export、façade / Published Language、port 所有权、Composition Root 接线与机械守卫共同证明。

- 同名目录、文件位置或树形对称性 **NEVER** 单独证明依赖向内、出站 port 所有权、入站 façade 所有权或内部类型未泄漏。
- 能力 **MUST** 先按稳定职责命名；内部 **SHOULD** 保持扁平或按共同变化的用例共置。model、技术目录与 crate 在符合各自证据时才 **MAY** 引入；port **MUST** 独立遵守 R2：稳定策略若不抽象就会依赖易变 detail 时 **MUST** 引入，否则 **NEVER** 预建。完整判据见 [06-code-organization.md](06-code-organization.md)。
- 仅移动或重命名目录而未改变 import、可见性、公开面与装配图，**NEVER** 视为完成架构边界调整。
- 架构评审与守卫 **MUST** 检查实际依赖和公开面，**NEVER** 以目录命名匹配替代这些证据。

## 4. Agent 执行生命周期状态机原则

- **MUST** `Run` 是全系统唯一的 **Agent 执行生命周期状态机**，位于 Agent Runtime 内，且**内存态、不持久化、崩溃从头开始**。
- **MAY** 其他 BC 为自身局部聚合定义状态机（例如 Task 状态迁移、Workflow effort 调节、MCP Connection 生命周期），但这些状态机 **NEVER** 复制、驱动或替代 Run 的执行生命周期。
- Session **NEVER** 拥有独立状态机——Session 是数据聚合（对话历史容器），其“状态”是 Run 状态的投影或 IO 动作，无独立执行生命周期不变量。
- 系统 **NEVER** 引入 durable model invocation checkpoint 链（人在环 CLI 由“人 + 文件系统真实状态”兜底副作用一致性）。
- Reasoning Node 状态机（Workflow）是 **effort 调节机**，与 Run **执行状态机**职责分离，NEVER 混淆。

## 5. Future 演进的依赖约束

| 演进 | 约束 |
|---|---|
| Server 化 | 传输层（WS / 进程拓扑）NEVER 进核心；`AgentClient` 保持传输透明 |
| 单 main + 多 sub（v0.2.0） | 由 Agent Runtime 的 SubAgent 能力承担（多个子 Run），不属编排、不新增 BC；无多-agent 图编排的长期计划 |

## 6. 守卫可执行性

每条铁律 **MUST** 配套可在 CI / Stop hook 运行的机械守卫后才算可执行。具体已覆盖项 **MUST** 以 [架构守卫注册表](../03-engineering/architecture-guards.md) 为真相源，待迁移项与覆盖缺口 **MUST** 由 [迁移治理](../03-engineering/migration-governance.md) 跟踪；本文 **NEVER** 宣称具体覆盖状态。

## 7. 相关文档

- 系统架构：[04-system-architecture.md](04-system-architecture.md)
- 代码组织规范：[06-code-organization.md](06-code-organization.md)
- 上下文地图：[03-context-map.md](03-context-map.md)
- 架构守卫注册表：[../03-engineering/architecture-guards.md](../03-engineering/architecture-guards.md)
- 迁移治理：[../03-engineering/migration-governance.md](../03-engineering/migration-governance.md)
- 目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：依赖方向总则、7 条依赖铁律、固定目录模板重定位、单状态机原则 | #760 |
| 2026-07-11 | 违反示例通用化（移除具体现有类型名）、固定目录表目标态化、文档引用链接化、新增修改历史 | #760 |
| 2026-07-11 | 术语改名：Agent Execution→Agent Runtime、AgentRun→Run | #760 |
| 2026-07-11 | Future 约束去"编排器"表述，改为 SubAgent 承担 v0.2.0 单 main + 多 sub | #760 |
| 2026-07-12 | 将“全系统单状态机”精确化为“Run 是唯一 Agent 执行生命周期状态机”，允许各 BC 局部聚合状态机 | #743 / #787 |
| 2026-07-14 | 以 external detail → capability policy 重写依赖语义，明确按证据强制 port、Context Map 的 ACL / Shared Kernel 例外、目录非证据原则，以及守卫覆盖状态的真相源职责 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
