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

模型调用的具体战术端口签名 **MUST** 以 [Runtime 端口与适配器](../02-modules/runtime/06-ports-and-adapters.md) 为唯一真相源：`ProviderPort` 是 Runtime-owned 消费方 outbound SPI，其签名由 Runtime 定义并唯一登记；[Provider 模块设计](../02-modules/provider/README.md) 只描述 Provider adapter 如何实现该签名，**NEVER** 被视为签名真相源。工作区交互的具体战术端口名称和职责 **MUST** 以 [Project 端口与适配器](../02-modules/project/02-ports-and-adapters.md) 为真相源（Project 作为该关系的供应方拥有并发布该入站 façade）；本图 **NEVER** 另行命名战术 port。

- 稳定能力策略 **NEVER** import、构造或泄漏易变外部 detail 类型；外部 detail **MUST** 在边界完成 wire type、错误与事件转换。
- 当确认稳定策略若不抽象就会依赖易变 detail 时，该策略 **MUST** 在真实易变边界定义并拥有目的性**出站 port**；port **SHOULD** 靠近消费外部交互的用例。供其他能力调用的**入站 façade / OHS** **MUST** 由供应能力拥有。
- 尚未形成策略 / detail 边界时，能力 **MAY** 保持私有具体依赖，但**该私有具体 detail 只能由持有它的 adapter / detail 内部代码使用**，能力的稳定策略层 **NEVER** 依赖它——即便尚无正式 port，策略与 detail 的内部分离仍 **MUST** 成立；该依赖也 **NEVER** 越过能力 façade，**NEVER** 成为跨能力契约。
- **技术外部 seam 与 BC boundary seam 判据不同、NEVER 混用**：网络、文件系统、进程、时钟等易变**技术外部 detail** 的出站 port 由消费该细节的能力策略按技术证据（见 [06-code-organization.md §3.4](06-code-organization.md#34-可选-port)）定义并拥有；两个 Bounded Context 之间的业务关系（**BC boundary seam**）**NEVER** 套用该技术证据，而是由 [Context Map](03-context-map.md) 登记的供应方拥有入站 OHS / façade 并发布稳定语言。当供应方 BC 恰好需要吸收易变外部技术差异时（例如 Provider 吸收各家 LLM API 差异），该技术细节仍由**消费方**定义并拥有隔离它的出站 port（如 Runtime-owned `ProviderPort`），供应方只负责实现——技术外部 seam 与 BC 关系可在同一条边上共存，但所有权判据 **NEVER** 混用。例如 Storage：其具体存储技术（driver，如 sled / 文件系统）是 Storage **私有**的技术外部 seam detail，只由 Storage 自己定义并拥有一个私有 backend SPI 隔离它；Storage 对外发布给其他数据 BC 的 `AtomicBlob` / `Dataset` OHS 是另一条**不同方向**的 BC boundary seam——它是被消费方 integration adapter **调用**的稳定入站服务，**NEVER** 是由 driver 实现的 SPI，二者所有权判据 **NEVER** 混同。
- 具体生产实现 **MUST** 只由 Composition Root 选择并绑定。为隐藏模块私有 detail，Composition **MAY** 调用能力发布的 opaque production factory；factory **MUST** 只完成模块内部构造，**NEVER** 自行读取全局配置、在候选实现间决策或成为第二个业务装配入口；**factory 只限生产代码路径**，单元 / 集成测试 **MAY** 绕过 factory 与 Composition Root，直接构造轻量 fake / stub 实现注入被测策略。

## 2. 依赖铁律（强制）

| # | 铁律 | 违反模式 |
|---|---|---|
| R1 | **能力策略 NEVER 依赖外部 detail**：稳定策略 NEVER import、构造或公开易变技术实现类型 | 用例策略 import 某个 provider driver 的具体类型 |
| R2 | **隔离真实易变 detail 的出站 port MUST 由消费策略定义**：port 按交互目的命名，外部 detail 依赖并实现它；供其他能力调用的入站 façade / OHS 由供应能力拥有；没有真实 seam 时 NEVER 预建 | HTTP driver 发布核心调用 trait，或消费方重新包装 Project-owned OHS |
| R3 | **跨能力 MUST 只经窄 façade 或 Published Language 通信**，NEVER 直接 import 对方内部类型；合法跨能力依赖面包含：① 供应方发布的入站 façade / OHS 或 Published Language；② 外部 detail 或供应方 adapter 实现消费方拥有的出站 SPI（例如 Provider adapter 实现 Runtime-owned `ProviderPort`）——该方向由消费方定义签名、供应方 / 外部 adapter 实现，不属于"直接 import 对方内部类型"，也不与 Provider 的 ACL 实现冲突 | 一个能力直接 import 另一个能力的内部存储结构，或消费方反向依赖供应方 adapter 内部 wire type |
| R4 | **外部驱动 detail NEVER 触碰能力内部**：TUI / CLI 只经 `AgentClient`，NEVER import Runtime 内部类型 | UI 里出现 Runtime 的内部上下文类型 |
| R5 | **Config 单向下发**：所有 BC 顺从消费只读 `ConfigSnapshot`，NEVER 反向依赖，NEVER 绕过快照读裸配置 / env 散点 | 业务代码里直接读环境变量 |
| R6 | **Composition Root MUST 是唯一生产装配入口**：具体实现选择、factory 调用与跨能力接线只由组合根发起；模块可在 composition-only opaque factory 内构造私有 detail，但 NEVER 自行选择候选实现或从业务路径触发生产装配；“唯一”按每个 deployable production assembly（可独立构建 / 部署的生产制品）解释——同一 deployable 内 MUST 只有一个组合根，不同 deployable（如未来 Server 化的控制面与 worker）MAY 各自拥有独立的唯一组合根 | 能力在业务路径直接 `new` 存储实现，或根据全局配置自行挑选 adapter，或同一 deployable 内出现第二个装配入口 |
| R7 | **边界模型 MUST 按 Context Map 转换或共享**：provider wire type **MUST** 经 Provider ACL 转为稳定 `Message`；`DomainEvent` **MUST** 经 TUI ACL 转为 TUI Model；只有 [Context Map](03-context-map.md) 登记的 Shared Kernel 类型 **MAY** 在其列明的参与 BC 间跨界（`Message` 用于 Runtime / Context Management / Provider，`ReasoningLevel` 用于 Workflow / Config / Runtime / Context Management / Provider），其他内部类型 **NEVER** 直通 | provider wire type 直接进入 Runtime，或 `DomainEvent` 直接进入 TUI Model，或未登记类型被多个 BC 各自复制 |
| R8 | **同 crate 内 Hexagonal 层级依赖方向**：feature crate 内部 **MUST** 采用 Hexagonal 依赖方向（`domain ← application ← ports ← adapters`）。`domain` **NEVER** 依赖 `application` / `ports` / `adapters`；`application` 只依赖 `domain`（+ 经 `ports` 消费外部）；`ports` 只依赖 `domain`；`adapters` 可依赖 `ports` + `domain`。小模块 MAY 只使用部分层，**NEVER** 为对称预建空层。采用 `capabilities/` 递归竖切时，每个 capability 内部仍 **MUST** 遵守此方向 | `domain` 层 `use crate::adapters::xxx`；`application` 层直接 `use crate::adapters::xxx` 绕过 `ports`；`ports` 层依赖 `adapters` |

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

每条铁律 **MUST** 配套可在 CI / Stop hook 运行的机械守卫后才算可执行。具体已覆盖项 **MUST** 以 [架构守卫注册表](../03-engineering/01-architecture-guards.md) 为真相源，待迁移项与覆盖缺口 **MUST** 由 [迁移治理](../03-engineering/03-migration-governance.md) 跟踪；本文 **NEVER** 宣称具体覆盖状态。

## 7. 相关文档

- 系统架构：[04-system-architecture.md](04-system-architecture.md)
- 代码组织规范：[06-code-organization.md](06-code-organization.md)
- 上下文地图：[03-context-map.md](03-context-map.md)
- 架构守卫注册表：[../03-engineering/01-architecture-guards.md](../03-engineering/01-architecture-guards.md)
- 迁移治理：[../03-engineering/03-migration-governance.md](../03-engineering/03-migration-governance.md)
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
| 2026-07-15 | 修复评审 #13-#15：模型调用战术端口签名真相源改指向 Runtime 端口与适配器；新增技术外部 seam 与 BC boundary seam 判据区分说明；R3 明确合法跨能力依赖面包含供应方 façade/PL 与外部/供应 adapter 实现消费方 owned outbound SPI | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-16 | 明确无 seam 时私有具体 detail 只能由 adapter/detail 内部使用、稳定策略层 NEVER 依赖；技术外部 seam 与 BC boundary seam 说明补充 Storage driver（私有 backend SPI）与 AtomicBlob/Dataset OHS（被消费方 adapter 调用的入站服务）的所有权对照示例；R6 明确 Composition Root “唯一”按 deployable production assembly 解释；factory 补充只限生产代码路径、测试可绕过直接构造 fake | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 新增 R8：同 crate 内 Hexagonal 层级依赖方向（`domain ← application ← ports ← adapters`），作为 crate 内部默认约束 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
