# 上下文地图（Context Map）

> 层级：01-system（系统级总体设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0
> 本文定义 15 个 Bounded Context 之间的集成关系、方向、端口与防腐边界。以核心域 **Agent Runtime** 为中心组织（hub-and-spoke）。**只描述目标态集成关系，不记录当前代码位置。**

## 1. 集成模式图例

| 缩写 | 全称 | 含义 |
|---|---|---|
| **C/S** | Customer/Supplier | 上下游依赖；下游（客户）能对上游（供应商）提需求、有话语权 |
| **CF** | Conformist | 下游只能顺从上游模型，无话语权 |
| **OHS** | Open Host Service | 上游提供稳定对外服务（端口 / API） |
| **PL** | Published Language | 公共契约语言（DTO / trait） |
| **ACL** | Anticorruption Layer | 防腐层：翻译上游模型，防污染自身领域 |
| **Pub/Sub** | Publish/Subscribe | 单向事件流；发布者不知谁消费、消费者不影响发布者 |
| **SK** | Shared Kernel | 少量共享内核类型 |

## 2. 上下文地图总览

```
                    ┌─────────── 入站适配器（交付层，非 BC） ───────────┐
                    │   CLI      TUI(+ACL)      REPL          Server     │
                    └───────────────────┬───────────────────────────────┘
                                        │  AgentClient（入站端口 = OHS + PL，所有权属核心域）
                                        ▼
   ┌──────────────────────────  核心域  ──────────────────────────┐
   │   Agent Runtime（唯一 Agent 执行生命周期状态机 Run；派生 / 执行 SubAgent） │
   └───────┬───────────────────────────────────────────────────────┘
           │ 出站端口（C/S，Runtime 是 Customer）
   ┌───────┼────────┬────────┬────────┬───────┬───────┬───────┬────────┐
   ▼       ▼        ▼        ▼        ▼       ▼       ▼       ▼        ▼
 Context Workflow Provider  Tool    Policy  Memory  Task  Project   Hook
 Mgmt   (reason.) (ACL)   &Skill&Cmd                                    
   │                                                            │
   ▼ 持久化（C/S）                                               ▼ Pub/Sub
 Storage  ◀── Memory / Task 也经此落盘                    Audit（含 Cost）
   ▲
   │ CF + PL（ConfigSnapshot）
 Config ───────────────────────────────▶ 所有 BC（全局上游）
                                          Logging（全局横切）
```

## 3. 入站边：交付层 → 核心

| 上游 | 下游 | 模式 | 端口 / 契约 |
|---|---|---|---|
| CLI / TUI | Agent Runtime | C/S + **PL** + **ACL**(TUI) | 入站端口 `AgentClient`；TUI 用 `AgentEventMapper` 把领域事件转 Model |
| REPL | Agent Runtime | C/S + **PL** | 旧交互模式（退役方向） |
| Server | Agent Runtime | C/S + **PL** | 同 `AgentClient`，WSS 透传，worker 内核心不改 |

> `AgentClient` trait + `ChatEvent / Command / Snapshot / Error` = **入站 Published Language**，**所有权属 Agent Runtime 核心域**；独立成 SDK 契约 crate 仅为依赖倒置（clients 依赖契约而非核心实现）。

## 4. 核心出站：Agent Runtime → 各支撑 / 通用 BC

| 下游 BC | 模式 | 出站端口 | 说明 |
|---|---|---|---|
| Context Management | C/S | `ContextPort` | Runtime 请求"构建本轮 Context Window"（取历史 + compact + 注入 + prompt），CM 提供 OHS |
| Workflow | C/S | `ReasoningPort` | Runtime 询问当前 reasoning effort（reasoning graph 观察 tool 类型 / 结果调节）；Workflow **NEVER** 阻塞 loop 或强制流程，仅作 effort 调节器 |
| Provider | C/S + **ACL**(在 Provider 内) | `ProviderPort` | Provider 内部 ACL 吸收各家 LLM 差异，对 Runtime 暴露统一调用 + 流 |
| Tool & Skill & Command | C/S | `ToolCatalogPort` + `ToolExecutionPort`；Skill/Command 独立端口 | Tool 目录与函数调用分离；Skill 物化 PromptFragment；Command 按 PromptInjection / SnapshotQuery / ApplicationControl 路由；MCP 是 Tool adapter |
| Policy | C/S | `PolicyPort` | 工具执行前的权限判断（Interaction approval gate 上游） |
| Memory | C/S | `MemoryPort` | 检索注入 + 反思写入（Reflection 产出 Memory Suggestion） |
| Task Management | C/S | `TaskPort` | Runtime 读写 Task 规划自身工作；Task 拥有状态机 + 依赖图不变量 |
| Project / Workspace | C/S | `WorkspacePort` | worktree 进出、git 上下文供给（含 git context 注入的数据源） |
| Hook | C/S | `HookPort` | 生命周期点触发 hook |
| Audit | **Pub/Sub**（Runtime 是 Supplier of events） | `AuditSink` | Runtime 发执行 / 成本事件，Audit 顺从消费（含 Cost / Usage） |

> **SubAgent 不在此表**：SubAgent 的派生与执行是 Agent Runtime 的核心能力（子 Run 也是 Runtime 的执行实例），不是一条对外端口。

## 5. 全局上游：Config → 所有 BC

| 上游 | 下游 | 模式 | 契约 |
|---|---|---|---|
| Config | 全部 BC | **CF + PL** | 各 BC 顺从消费只读 `ConfigSnapshot`（Published Language），**NEVER** 反向依赖，**NEVER** 绕过快照读裸配置 |

## 6. 持久化：数据 BC → Storage

| 上游 | 下游 | 模式 | 说明 |
|---|---|---|---|
| Context Management / Memory / Task | Storage | C/S | Storage 提供原子写 / 损坏兜底**机制**，不拥有数据本体。**Session 落盘时内嵌 Task / Project 快照**（经端口收集，恢复时分发回去）——跨 BC 快照组装，边界不破。 |

## 7. Shared Kernel（谨慎，尽量小）

| 共享内核 | 参与 BC | 风险控制 |
|---|---|---|
| `Message`（对话消息类型） | Agent Runtime / Context Management / Provider | 最小化，只放稳定核心类型 |
| `ID`（UUIDv7 newtype） | 全域 | 纯标识，无行为 |
| `Task` 类型 | 实为 **Task BC 的 Published Language**（非 SK），其他 BC 引用其发布类型 | 不变量由 Task BC 独占 |

## 8. 关键 ACL 位置（防腐重点）

1. **Provider 内部**：各家 LLM API → 统一领域调用与 `Message`（最重的 ACL）。
2. **TUI `AgentEventMapper`**：领域事件 → TUI Model / Msg，防核心内部类型泄漏进 UI。
3. **Session 快照组装**：Context Management 落盘 Session 时从 Task / Project 收快照，恢复时分发——经端口，不共享内部结构。

## 9. 未来演进的地图预留

| 演进 | 版本 | 地图影响 |
|---|---|---|
| **Server 化** | v0.1.0 之后 | Server 作为入站适配器接 `AgentClient`；**核心域对传输层透明**（进程内直调 / WS 远程二选一，核心不改）。 |
| **单 main + 多 sub** | v0.2.0 | 由 Agent Runtime 的 SubAgent 能力承担（多个子 Run），**不属编排**，不新增 BC。无多-agent 图编排的长期计划。 |

## 10. 三条 Context Map 决策

1. **Audit = Pub/Sub 单向事件**：Runtime 只管 emit，不依赖 Audit，Audit 不影响 Runtime。
2. **Interaction 不成 BC**：ask_user / 权限审批 / plan mode / pause-resume 是 Runtime 的用例族，经 `InteractionPort` + `PolicyPort` 协作，由不同触发源（tool / policy / user）复用。
3. **Task 类型 = Task BC 的 Published Language**（非 Shared Kernel）：由 Task BC 独占不变量，其他 BC 引用其发布类型。

## 11. 相关文档

- 产品与子域：[01-product-and-domain.md](01-product-and-domain.md)
- 统一语言：[02-ubiquitous-language.md](02-ubiquitous-language.md)
- 系统架构与六边形：[04-system-architecture.md](04-system-architecture.md)
- 依赖规则：[05-dependency-rules.md](05-dependency-rules.md)
- 目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：集成模式、入站 / 出站边、Shared Kernel、ACL、Future 预留 | #760 |
| 2026-07-11 | 清理 crate 路径引用改为纯目标态、文档引用链接化、新增修改历史 | #760 |
| 2026-07-11 | 术语改名：Agent Execution→Agent Runtime、AgentRun→Run、缩写 AE→Runtime | #760 |
| 2026-07-11 | Workflow 降为支撑域：移出核心框、并入 §4 出站端口表（ReasoningPort）、删原"核心内部"节、Future 去多-agent 编排 | #760 |
| 2026-07-12 | Tool BC 出站契约拆为 Catalog/Execution 双端口；Skill 与 Command 使用独立端口，MCP 定位为 Tool adapter | #787 |
| 2026-07-12 | 将 Run 限定为唯一 Agent 执行生命周期状态机，与各 BC 局部聚合状态机区分 | #743 / #787 |
