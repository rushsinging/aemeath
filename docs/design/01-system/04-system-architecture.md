# 系统架构

> 层级：01-system（系统级总体设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：[#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 aemeath 的整体架构形态、能力边界、选择性 Hexagonal seam、组合根装配原则与 crate 映射。**只描述目标态，不记录当前代码状态或迁移进度。** 依赖方向的强制规则见 [05-dependency-rules.md](05-dependency-rules.md)，代码组织判据见 [06-code-organization.md](06-code-organization.md)。

## 1. 架构决策

> **capability-first modular monolith + use-case colocation + ports on demand**
> DDD 战略设计识别稳定能力边界，选择性 Hexagonal seam 隔离真实内外部交互，Clean 规则规定源码依赖方向，唯一 Composition Root 负责生产装配。

- **MUST** 用 DDD 战略设计识别子域、统一语言、Bounded Context 与 Context Map（见 [01](01-product-and-domain.md)~[03](03-context-map.md)）。
- 仓库与 crate 内部 **MUST** 先按稳定业务能力组织，并通过窄 façade 表达各能力的公开面；同一用例共同变化的校验、编排与局部转换 **SHOULD** 共置。
- 当稳定能力策略若不抽象就会依赖易变外部 detail 时，该策略 **MUST** 定义并拥有按交互目的命名的**出站 port**；没有真实 seam 时 **NEVER** 预建。供其他能力调用的**入站 façade / OHS** 则 **MUST** 由提供能力的一方拥有并发布稳定语言。
- 源码依赖 **MUST** 遵循 Clean 方向，从易变外部 detail 指向稳定 capability policy；策略 **NEVER** 反向依赖具体技术实现。
- 跨 Bounded Context 交互 **MUST** 使用 [Context Map](03-context-map.md) 为该关系定义的集成模式及稳定 façade / Published Language，**NEVER** 穿透对方内部类型；只有关系两侧的模型需要翻译时，该边界才 **MUST** 提供 ACL。
- 架构 **NEVER** 要求每个能力复制相同的横向层目录；具体结构 **MUST** 按 [代码组织规范](06-code-organization.md) 的复杂度证据选择。
- **MUST** 先在单 crate 内用 Rust 可见性稳定能力边界；只有满足[代码组织规范 §3.6](06-code-organization.md#36-可选-crate) 的强边界收益时才 **MAY** 拆 crate。
- **MUST** 保持 Composition Root 为唯一生产装配入口。

## 2. 能力策略与选择性 Hexagonal seam

```text
源码依赖方向（箭头）：external detail -> capability policy

CLI / TUI / REPL / Server(Future) ──依赖 AgentClient──────▶ Agent Runtime 能力策略
Provider HTTP / SSE       ──实现 Runtime 消费方拥有的模型调用 port──▶ Model Invocation 用例策略
Git CLI / 子进程 detail    ──实现 Project 消费方拥有的 git worktree port──▶ Project Workspace 能力策略
Storage 内部技术 driver（sled / file 等） ──实现 Storage 私有 backend SPI──▶ Storage 机制策略（发布 AtomicBlob/Dataset OHS）
持久化 integration adapter ──调用 Storage-owned OHS 并实现消费方 repository / sink port──▶ Context Management / Memory / Audit 等能力策略
```

图中箭头表示源码依赖，不表示运行时调用方向。隔离易变 detail 的出站 port 属于消费该外部交互的稳定策略；外部 detail 依赖或实现该 port，由 Composition Root 选择生产实现。Storage 是独立供应能力，但其内部区分两层，**NEVER** 混用：① 具体存储技术（driver，例如 sled / 文件系统）是 Storage **私有**的易变技术 detail，只由 Storage 自己按技术外部 seam 判据定义并拥有一个私有 backend SPI 隔离它，driver 只在 Storage 内部实现该私有 SPI，**NEVER** 对外可见，Storage 的机制策略与所有跨 BC 消费方 **NEVER** 依赖具体 driver 类型；② Storage 对外发布的 `AtomicBlob` / `AtomicDataset` OHS 是稳定**入站**服务，由消费方 integration adapter **调用**（而非由 driver 实现）——OHS 与私有 backend SPI 是两个不同方向、不同所有权的端口，**NEVER** 把二者混同为同一个"driver 实现的 SPI"。消费方拥有窄 repository / sink port，integration adapter 同时实现该消费方 port 并调用 Storage OHS，**NEVER** 把 Storage driver 直接伪装成所有数据 BC 的 port。Task / Project 快照只由 Context Management 组装进 Session 后经其 repository 落盘，**NEVER** 从该示意推导独立的 Task / Project → Storage 路径。供其他能力调用的入站 façade / OHS 属于供应方，例如 Runtime-owned `AgentClient` 与 Project-owned `WorkspaceRead` / `WorkspaceControl` / `WorkspacePersist`。键盘、WebSocket、HTTP、文件、git、进程与 runtime 等技术类型 **MUST** 在 seam 外侧完成转换，**NEVER** 越过能力 façade。

模型调用的具体战术端口签名 **MUST** 以 [Runtime 端口与适配器](../02-modules/runtime/06-ports-and-adapters.md) 为唯一真相源：`ProviderPort` 是 Runtime-owned 消费方 outbound SPI，其签名由 Runtime 定义并唯一登记；[Provider 模块设计](../02-modules/provider/README.md) 只描述 Provider adapter 如何实现该签名与内部 ACL，**NEVER** 被视为签名真相源、**NEVER** 另行复制或改写该签名。工作区交互的具体战术端口名称和职责 **MUST** 以 [Project 端口与适配器](../02-modules/project/02-ports-and-adapters.md) 为真相源（Project 作为该关系的供应方拥有并发布该入站 façade）；系统级示意 **NEVER** 另行命名战术 port。

Hexagonal 在这里是按需使用的 inside / outside 隔离：已形成稳定策略与易变 detail 边界时 **MUST** 引入目的性 port；尚未形成该边界时，能力 **MAY** 保持私有具体依赖。目录位置、文件数量或对称性 **NEVER** 单独构成 port 的理由。

## 3. 组合根（Composition Root）

- **唯一生产装配入口**：单一的 composition 模块负责把所有 BC 与适配器接线成一个可运行系统；**“唯一”按每个 deployable production assembly（可独立构建 / 部署的生产制品，例如今天的单一 CLI/TUI 二进制，未来 Server 化后的控制面与 worker 各自二进制）解释**——同一 deployable 内部 **MUST** 只有一个 composition 入口，不同 deployable **MAY** 各自拥有自己独立的唯一组合根；跨 deployable 存在多个组合根 **NEVER** 被视为违反本条，但同一 deployable 内新增第二个装配入口 **MUST** 视为违反。
- **依赖注入方式**：trait 对象 `Arc<dyn Port>`（动态分发），构造期注入。
- **装配职责**：能力之间的接线与所有已引入 seam 的生产实现选择 **MUST** 收敛在组合根一处完成；能力与外部 detail 只声明、消费或实现 port，**NEVER** 根据全局配置或运行环境自行选择生产实现。
- 为保持模块私有性，能力 **MAY** 暴露 composition-only 的 opaque production factory，并在 factory 内构造不跨模块公开的私有 detail；Composition Root **MUST** 独占“选择并调用哪个 factory”的权力。若实现选择需要配置或存在多个候选，选择逻辑 **MUST** 回到 Composition Root，或由它以稳定 factory 参数显式下发。**Factory 只限生产代码路径使用**：单元 / 集成测试 **MAY** 绕过 factory 与 Composition Root，直接在测试代码内构造轻量 fake / stub 实现并注入被测策略，**NEVER** 因“唯一装配入口”而要求测试也经生产 factory 或组合根装配。
- 核心能力与外部 detail **NEVER** 在业务执行路径私自 `new` 具体生产实现绕过组合根；模块私有 factory 的内部构造不构成第二个装配入口。

## 4. 能力与 crate 映射原则（Screaming Architecture）

目录 / crate 名 **SHOULD** “喊出业务能力”，而不是先表达技术分类。下表描述系统级角色，**NEVER** 规定单个能力必须拥有对应子目录：

| 角色 | 承载 |
|---|---|
| 外部驱动 detail | CLI + TUI + REPL |
| Composition Root | 唯一生产装配 |
| 核心 / 支撑能力 | Agent Runtime / Workflow / Context Management / Memory / Task / Project / Policy / Audit / Tool&Skill&Command / Provider / Hook / Storage / Application Version Control 等 |
| 横切 / 共享内核 | Config、经证明的共享类型、最小内核 |
| Published Language | 稳定入站契约与 SDK 类型 |
| 通用基础设施 | 有明确 owner、经证明跨能力共享的通用基础设施 / 工具 |

**BC 与 crate 不强制 1:1**：

- 一个 crate 可含多个 BC 的落点；一个 BC 可跨多个 crate（如 Context Management 跨核心与 prompt 能力；Task 跨类型定义、持久化与工具适配）。
- 能力内部 **MUST** 先按 [代码组织规范](06-code-organization.md) 选择扁平模块或内聚用例子模块，**NEVER** 为统一外观预建横向目录。
- **判据**：先在单 crate 内用 Rust module privacy 稳定边界；crate 升格 **MUST** 直接采用[代码组织规范 §3.6](06-code-organization.md#36-可选-crate) 的完整强边界收益与提案责任，**NEVER** 在本文复制其子集。

## 5. Agent Runtime 系统级不变量

- `Run` **MUST** 是唯一的 Agent 执行生命周期状态机；其他 BC 的局部状态机 **NEVER** 复制、驱动或替代它。
- Main / SubAgent **MUST** 共享同一套 loop 骨架；角色差异 **MUST** 由输入上下文与运行规格表达，**NEVER** 复制第二套循环。
- 模型调用、工具协调、上下文协调、交互与事件投影职责 **MUST** 分离，并由 loop 骨架统一协调。
- Agent Runtime 的具体模块名称、内部依赖拓扑与 port 消费映射 **MUST** 只以 [Runtime 模块边界](../02-modules/runtime/02-module-boundaries.md) 为战术真相源；本文 **NEVER** 复制其模块树。

## 6. 传输透明原则（Server 化预留）

- **MUST** 核心域对传输层透明：`AgentClient` 既可进程内直调（TUI），也可经 WS 远程（Server），核心不改。
- Agent Runtime **NEVER** 感知 WS / 进程拓扑 / 序列化细节。
- Server 化时新增独立的协议 crate 与 server 应用（控制面 + worker），均为适配器，不进核心。

## 7. 相关文档

- 产品与子域：[01-product-and-domain.md](01-product-and-domain.md)
- 统一语言：[02-ubiquitous-language.md](02-ubiquitous-language.md)
- 上下文地图：[03-context-map.md](03-context-map.md)
- 依赖规则与铁律：[05-dependency-rules.md](05-dependency-rules.md)
- 代码组织规范：[06-code-organization.md](06-code-organization.md)
- 目录总览：[../README.md](../README.md)
- 模块级设计：[../02-modules/README.md](../02-modules/README.md)
- 横切工程：[../03-engineering/README.md](../03-engineering/README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：架构决策、六边形形态、组合根、crate 映射、内部模块、传输透明 | #760 |
| 2026-07-11 | 移除组合根现状 / TODO 描述改为目标态、crate 映射去"当前"措辞、文档引用链接化、新增修改历史 | #760 |
| 2026-07-11 | 术语改名：Agent Execution→Agent Runtime、AgentRun→Run | #760 |
| 2026-07-11 | Workflow 从核心域挪到支撑域（六边形图） | #760 |
| 2026-07-12 | Tool Coordination 对齐 Catalog/Execution 双端口及 Runtime 编排职责 | #787 |
| 2026-07-12 | Run 状态机表述限定为 Agent 执行生命周期，允许其他 BC 局部聚合状态机 | #743 / #787 |
| 2026-07-14 | 架构总决策改为 capability-first + 用例共置 + 按需 port，以 Context Map 约束跨 BC 集成，区分 DDD 战略边界与战术聚合，将 crate 升格判据收敛到代码组织规范，并将 Runtime 具体拆分收敛到模块级真相源 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 修复评审 #12-#13：§2 示意图标注 Server 为 Future；模型调用战术端口签名真相源由 Provider 模块设计改为 Runtime 端口与适配器（`ProviderPort` 是消费方 outbound SPI，Provider 只描述实现） | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-16 | §2 修正 Storage 示意：driver 是 Storage 私有 backend SPI 的实现，AtomicBlob/Dataset OHS 是被消费方 integration adapter 调用的入站服务，两者所有权与方向不再混同；§3 补充 Composition Root “唯一”按 deployable production assembly 解释，并明确 opaque production factory 只限生产代码路径，测试可绕过直接构造 fake | [#972](https://github.com/rushsinging/aemeath/issues/972) |
