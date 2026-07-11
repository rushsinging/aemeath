# 系统架构

> 层级：01-system（系统级总体设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0
> 本文定义 aemeath 的整体架构形态、六边形端口结构、组合根装配原则与 crate 映射。**只描述目标态，不记录当前代码状态。** 依赖方向的强制规则见 [05-dependency-rules.md](05-dependency-rules.md)。

## 1. 架构决策

> **DDD-guided Modular Monolith with Clean Architecture and Hexagonal Ports & Adapters**
> 用 DDD 识别边界，用 Screaming Architecture 表达业务能力，Application 层按需 Vertical Slice，通过 OHS + Published Language + ACL 管理上下文集成。

- **MUST** 用 DDD 识别子域 / 统一语言 / BC / 聚合 / Context Map（见 [01](01-product-and-domain.md)~[03](03-context-map.md)）。
- **MUST** 用 Hexagonal 定义入站 / 出站 Port，Adapter 隔离 TUI / Provider / Tool / Storage / FS / Tokio。
- **MUST** 遵循 Clean 依赖规则：依赖只指向业务核心。
- **MUST NOT** 把 COLA 目录模板（`contract / gateway / business`）当作 DDD 概念或强制结构（见 [05-dependency-rules.md](05-dependency-rules.md) 的重定位说明）。
- **MUST** 先在单 crate 内稳定模块边界，再按语言 / 不变量 / 生命周期判断是否拆 crate。
- **MUST** 保持 Composition Root 为唯一生产装配入口。

## 2. 六边形形态

```
        ┌──────────────── 入站适配器（Driving / Primary）────────────────┐
        │   CLI    │   TUI（TEA + AgentEventMapper ACL）  │  REPL  │ Server │
        └────────────────────────────┬───────────────────────────────────┘
                                      │ 入站端口 AgentClient
                                      ▼
        ╔══════════════════ 应用核心（业务） ══════════════════╗
        ║  核心域：Agent Execution · Workflow                   ║
        ║  支撑域：Context Mgmt · Memory · Task · Project ·      ║
        ║          Policy · Audit · Tool&Skill&Command          ║
        ║  （领域模型 + 应用服务，纯业务、依赖向内）              ║
        ╚═══════════════════════════┬══════════════════════════╝
                                     │ 出站端口 *Port
        ┌────────────────────────────┼────────────────────────────┐
        │  Provider  Storage  Git(Workspace)  Hook  Logging ...    │
        │        出站适配器（Driven / Secondary）                   │
        └──────────────────────────────────────────────────────────┘
```

- **入站适配器**：驱动核心，把外部输入（键盘 / WS / CLI 参数）翻译为 `AgentClient` 调用。
- **应用核心**：领域模型 + 应用服务（用例编排），**不依赖任何适配器**。
- **出站适配器**：实现核心声明的出站端口（`ProviderPort` / `StoragePort` / `WorkspacePort` / `HookPort` …），把技术细节（HTTP / 文件 / git / tokio）挂在外侧、可插拔。

## 3. 组合根（Composition Root）

- **唯一生产装配入口**：单一的 composition 模块负责把所有 BC 与适配器接线成一个可运行系统。
- **依赖注入方式**：trait 对象 `Arc<dyn Port>`（动态分发），构造期注入。
- **装配职责**：所有 BC 的接线（Provider / Tool / Storage / 各出站端口的实现绑定）**MUST** 收敛在组合根一处完成；核心与适配器只声明与消费端口，不自行装配。
- **MUST NOT** 在核心或适配器内部私自 `new` 具体实现绕过组合根。

## 4. crate 映射原则（Screaming Architecture）

目录 / crate 名应"喊出业务能力"，而非技术分层。目标映射：

| 层 | 承载 |
|---|---|
| 入站适配器 | CLI + TUI + REPL |
| 组合根 | 唯一装配 |
| 核心 / 支撑 BC | 各业务能力（Agent Execution / Workflow / Context Management / Memory / Task / Project / Policy / Audit / Tool&Skill&Command / Provider / Hook / Storage / Application Version Control 等） |
| 横切 / 共享内核 | Config、共享类型、最小内核 |
| 契约 | 入站端口 + Published Language（SDK） |
| 通用 | 日志、通用工具 |

**BC 与 crate 不强制 1:1**：
- 一个 crate 可含多个 BC 的落点；一个 BC 可跨多个 crate（如 Context Management 跨核心与 prompt 能力；Task 跨类型定义、持久化与工具适配）。
- **判据**：先在单 crate 内用模块（`mod`）稳定 BC 边界，只有当语言 / 不变量 / 生命周期确实独立时，才升格为独立 crate。

## 5. Agent Execution 内部模块（战术拆分）

核心域最复杂，内部按关注点拆分：

```
Agent Execution
├── AgentRun 聚合 + 状态机     # 唯一状态机，内存态
├── Loop Engine                # ReAct 循环骨架 + 停止条件（Main/SubAgent 共用）
├── Model Invocation 协调       # 调 ProviderPort，组装流式响应
├── Tool Coordination          # 双 ID 映射、并发执行、结果回收 → ToolPort
├── Context Coordination       # compact / 注入 / prompt → ContextPort
├── Interaction                # AwaitingUser / ApprovalGate → InteractionPort
└── Event Projection           # 领域事件 → SDK ChatEvent
```

> Loop 的复杂性通过"骨架 + 各 Coordinator + 端口下沉"化解，各关注点归对应 BC，见 [03-context-map.md](03-context-map.md)。

## 6. 传输透明原则（Server 化预留）

- **MUST** 核心域对传输层透明：`AgentClient` 既可进程内直调（TUI），也可经 WS 远程（Server），核心不改。
- **MUST NOT** 让 Agent Execution 感知 WS / 进程拓扑 / 序列化细节。
- Server 化时新增独立的协议 crate 与 server 应用（控制面 + worker），均为适配器，不进核心。

## 7. 相关文档

- 产品与子域：[01-product-and-domain.md](01-product-and-domain.md)
- 统一语言：[02-ubiquitous-language.md](02-ubiquitous-language.md)
- 上下文地图：[03-context-map.md](03-context-map.md)
- 依赖规则与铁律：[05-dependency-rules.md](05-dependency-rules.md)
- 目录总览：[../README.md](../README.md)
- 模块级设计：[../02-modules/README.md](../02-modules/README.md)
- 横切工程：[../03-engineering/README.md](../03-engineering/README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：架构决策、六边形形态、组合根、crate 映射、内部模块、传输透明 | #760 |
| 2026-07-11 | 移除组合根现状 / TODO 描述改为目标态、crate 映射去"当前"措辞、文档引用链接化、新增修改历史 | #760 |
