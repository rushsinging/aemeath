# 依赖规则与铁律

> 层级：01-system（系统级总体设计）
> 状态：Target（目标设计）｜对应 Issue：#760（S1）｜Milestone：v0.1.0
> 本文定义强制性的依赖方向规则。违反即架构腐化，由 `docs/design/02-architecture-guards.md` 的守卫脚本在 CI / Stop hook 拦截。

## 1. 依赖方向总则（Clean）

> **依赖只能自外向内指向业务核心。核心 NEVER 依赖外层。**

```
适配器（入站/出站）  ──依赖──▶  应用服务  ──依赖──▶  领域模型
        ▲                                              │
        └──────────── NEVER 反向 ──────────────────────┘
```

- **领域模型**（Entity / VO / Aggregate / 领域事件 / 端口 trait）：**零外部依赖**，不 import 任何适配器、框架、IO、tokio、ratatui、reqwest、serde_json 的具体使用（serde derive 允许）。
- **应用服务**（用例编排）：依赖领域模型 + 出站端口 trait，**不依赖端口的具体实现**。
- **适配器**：依赖端口 trait，提供实现；适配器之间 **NEVER** 互相依赖。

## 2. 依赖铁律（强制）

| # | 铁律 | 违反示例 |
|---|---|---|
| R1 | **核心 NEVER 依赖适配器**：`agent/features/*` 的领域 / 应用层 NEVER import `apps/cli`、provider driver 具体类型、storage 具体文件实现 | runtime 直接 `use provider::AnthropicDriver` |
| R2 | **端口定义在内、实现在外**：出站端口 trait 定义在核心域侧，具体实现在对应适配器 crate | `ProviderPort` trait 定义在 provider adapter 而非核心消费方 |
| R3 | **BC 之间只经端口 + PL 通信**，NEVER 直接 import 对方内部类型 | Context Management 直接 `use memory::internal::Store` |
| R4 | **入站适配器 NEVER 触碰 runtime 内部**：TUI / CLI 只经 `AgentClient`（sdk），NEVER `use runtime::...` 或 `use ...::runtime::api` | TUI 里出现 `runtime::api::ChatRuntimeContext` |
| R5 | **Config 单向下发**：所有 BC 顺从消费只读 `ConfigSnapshot`，NEVER 反向依赖 Config 消费方，NEVER 绕过快照读裸配置 / env 散点 | 业务代码里 `std::env::var("AEMEATH_...")` |
| R6 | **Composition Root 唯一装配**：具体实现的 `new` / 接线只在 `agent/composition`，NEVER 在核心或适配器内私自装配绕过组合根 | runtime 内部 `new` 一个 storage 实现 |
| R7 | **同名类型经 ACL 隔离**：领域 `Message` 与 provider 线格式、领域事件与 TUI Model，NEVER 跨界直用，MUST 经防腐层转换 | provider 的 JSON 消息直接进领域 |

## 3. COLA 目录的重定位说明

历史上部分 crate（如 `storage`、`runtime`）采用 COLA 分层目录 `contract / gateway / business / api`。**这是目录模板，不是 DDD 概念，不得当作架构语义**：

| COLA 目录 | 现状实际承担 | DDD 视角的正确理解 |
|---|---|---|
| `contract` | 极薄 marker + DTO 再导出 | 不是"领域契约"，多为空壳 |
| `gateway` | 对外门面类型再导出 | 近似"出站端口 + 适配器门面" |
| `business` | 真正的领域 / 应用实现 | 领域模型 + 应用服务 |
| `api` | `pub use` 统一出口 | crate 对外 facade |

- **MUST NOT** 因为存在 `contract/gateway/business` 目录，就认为它天然对应六边形的端口 / 适配器 / 领域分层。
- **迁移方向**：S5 按 BC 边界重整，用 Screaming Architecture 的能力命名替代机械 COLA 分层；端口 trait 显式化、领域模型下沉、适配器外置。

## 4. 单状态机原则

- **MUST** 全系统只有一个领域状态机：`AgentRun`（Agent Execution 内），**内存态、不持久化、崩溃从头开始**。
- **MUST NOT** 为 Session 建状态机——Session 是数据聚合（对话历史容器），其"状态"是 AgentRun 状态的投影或 IO 动作，无独立领域不变量。
- **MUST NOT** 引入 durable model invocation checkpoint 链（人在环 CLI 由"人 + 文件系统真实状态"兜底副作用一致性）。
- Reasoning Node 状态机（Workflow）是 **effort 调节机**，与 AgentRun **执行状态机**职责分离，NEVER 混淆。

## 5. Future 演进的依赖约束

| 演进 | 约束 |
|---|---|
| Server 化 | 传输层（WS / 进程拓扑）NEVER 进核心；`AgentClient` 保持传输透明 |
| Workflow（v0.2.0） | 编排器经"触发源"抽象驱动 AgentRun，NEVER 让 Agent Execution 反向依赖编排器内部 |

## 6. 守卫映射

以上铁律由 `docs/design/02-architecture-guards.md` 注册的守卫脚本机械拦截（详见该文件）。新增铁律 MUST 配套新增 / 调整守卫，否则规则形同虚设。

## 7. 相关文档

- 系统架构：`04-system-architecture.md`
- 上下文地图：`03-context-map.md`
- 架构守卫注册表：`../02-architecture-guards.md`
