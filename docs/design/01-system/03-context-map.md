# 上下文地图（Context Map）

> 层级：01-system（系统级总体设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：[#972](https://github.com/rushsinging/aemeath/issues/972)
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
           │ 支撑能力契约（C/S，Runtime 是 Customer）
   ┌───────┼────────┬────────┬────────┬───────┬───────┬────────┐
   ▼       ▼        ▼        ▼        ▼       ▼       ▼        ▼
 Context Workflow Provider  Tool    Policy  Memory  Task      Hook
 Mgmt   (reason.) (ACL)   &Skill&Cmd
   │                                                      │
   ▼ 持久化（C/S）                                         ▼ Pub/Sub
   Storage  ◀── Memory / Audit 也经此落盘                   Audit（Usage facts）
             Task 只由 Context Session 内嵌 snapshot 持久化

 Context Mgmt ── WorkspaceRead / WorkspacePersist ──▶ Project
 Tool ────────── WorkspaceRead / WorkspaceControl ──▶ Project
   ▲
   │ CF + PL（ConfigSnapshot）
 Config ───────────────────────────────▶ 所有 BC（全局上游）
                                          Logging（全局横切）
```

## 3. 入站边：交付层 → 核心

| 上游 | 下游 | 模式 | 端口 / 契约 |
|---|---|---|---|
| CLI / TUI | Agent Runtime | C/S + **PL** + **ACL**(TUI) | 入站端口 `AgentClient`；TUI 用 `AgentEventMapper` 把领域事件转 Model |
| Server | Agent Runtime | C/S + **PL** | 同 `AgentClient`，WSS 透传，worker 内核心不改 |

> `AgentClient` trait + `ChatEvent / Command / Snapshot / Error` = **入站 Published Language**，**所有权属 Agent Runtime 核心域**；独立成 SDK 契约 crate 仅为依赖倒置（clients 依赖契约而非核心实现）。

## 4. Runtime 依赖的支撑能力契约

下表描述 Runtime 作为 Customer 所依赖的契约，**NEVER** 暗示所有 trait 都归 Runtime 所有：供应 BC 对 Runtime 开放的 façade / OHS 归供应方所有；只有隔离 Runtime 策略与易变 detail 的出站 port 才归 Runtime 所有。

| 下游 BC | 模式 | 契约与所有权 | 说明 |
|---|---|---|---|
| Context Management | C/S | Context-owned `ContextPort` OHS | Runtime 请求"构建本轮 Context Window"（取历史 + compact + 注入 + prompt） |
| Workflow | C/S | Workflow-owned `ReasoningPort` OHS | Runtime 询问当前 reasoning effort（reasoning graph 观察 tool 类型 / 结果调节）；Workflow **NEVER** 阻塞 loop 或强制流程，仅作 effort 调节器 |
| Provider | C/S + **ACL**(在 Provider 内) | Runtime-owned outbound `ProviderPort` | Provider adapter 吸收各家 LLM 差异，实现 Runtime 定义的统一调用语言与有序流 |
| Tool & Skill & Command | C/S | Tool-owned `ToolCatalogPort` + `ToolExecutionPort` OHS；Skill / Command 各自发布窄 façade | Tool 目录与函数调用分离；Skill 物化 PromptFragment；Command 按 PromptInjection / SnapshotQuery / ApplicationControl 路由；MCP 是 Tool adapter |
| Policy | C/S | Policy-owned `PolicyPort` OHS | v0.1.0 只装配 `AllowAllPolicy`；Deny / RequireApproval 为接口预留，控制流仍归 Runtime |
| Memory | C/S | Memory-owned `MemoryPort` OHS | 检索注入 + 反思写入（Reflection 产出 Memory Suggestion） |
| Task Management | C/S | Task-owned `TaskAccess` / `TaskPersist` OHS | Runtime / Tool 只持 Access；Context Management 只持 Persist；同一 backing 守护状态机与依赖图不变量 |
| Hook | C/S | Hook-owned `HookPort` OHS | 一个类型化 façade；Hook 执行/重试归 Hook，触发时机和 directive 的 Run 状态迁移归调用方 |
| Application Version Control | C/S | Runtime-owned outbound `ApplicationVersionPort` | 该 seam 隔离 Runtime 的 Application Control policy 与版本模块的 source/cache/installer detail；CLI/TUI 不直接持有更新模块内部端口 |
| Audit | **Pub/Sub**（Runtime 是 Supplier） | Runtime-owned outbound `UsageSink`（MVP） | Runtime 非阻塞提交 Usage metadata；Audit adapter 独立持久化和查询，不影响 Runtime；Cost/Pricing 保留 Future |

> **SubAgent 不在此表**：SubAgent 的派生与执行是 Agent Runtime 的核心能力（子 Run 也是 Runtime 的执行实例），不是一条对外端口。

Interaction 同样不是第 16 个 BC：Runtime-owned `InteractionPort` 隔离 TUI / Server / parent-mediated request-reply detail。AskUser Tool 只返回 Tool-owned typed suspension，由 Runtime ACL 映射为 `InteractionRequest`、保存 continuation 并独占 `AwaitingUser` / resume 状态转换；Tool BC **NEVER** 再发布同义 `UserInteraction` port。

`WorkspaceMode` 是 `RunSpec` 的装配策略，不形成 Runtime → Project 出站边。Composition 在 active-main-session-slot scope 中保留 Project wiring：Main agent 启动时只选择一次 production wiring，同一 Session 的全部 Main Run 复用；运行期 resume 在排他 gate 内替换完整 state。Sub 由 composition-provided AgentDispatch 对父 scope 执行 isolated derivation；Runtime **NEVER** 持有 Project 端口或 wiring。

### 4.1 支撑 BC 之间的直接能力边

| 消费方 | 供应方 | 模式 | 端口 / 契约 | 说明 |
|---|---|---|---|---|
| Tool & Skill & Command | Project / Workspace | C/S + OHS | Project-owned `WorkspaceRead` / `WorkspaceControl` | Tool 按需直接消费窄能力；只读文件 Tool **MUST** 只获得 Read，Control **MUST** 只注入 Bash、EnterWorktree、ExitWorktree |
| Context Management | Project / Workspace | C/S + OHS | Project-owned `WorkspaceRead` / `WorkspacePersist` | Read 提供路径、分支等 Context Window 数据；Persist 专用于 Session 快照组装与恢复 |
| Memory | Project / Workspace | CF + PL | Project-owned `ProjectIdentity` | Composition 以 identity 选择项目级 Memory，并把同一 MemoryPort Arc 注入 Context / Runtime / MemoryTool；Memory **NEVER** 依赖 WorkspaceRead 或自行读取 cwd |

`WorkspaceRead` / `WorkspaceControl` / `WorkspacePersist` 是 Project 发布的稳定能力。Composition **MUST** 从同一个 composition-internal workspace scope 向 Context Management 与 Tool backing implementation 分发所需窄 view，保证 active Main session slot 跨多个 Run 复用同一实例、每个 Sub 使用各自隔离实例；该 scope 与 wiring **NEVER** 进入 Runtime、Tool 或 Context 类型。

## 5. 全局上游：Config → 所有 BC

| 上游 | 下游 | 模式 | 契约 |
|---|---|---|---|
| Config | 全部 BC | **CF + PL** | 各 BC 顺从消费只读 `ConfigSnapshot`（Published Language），**NEVER** 反向依赖，**NEVER** 绕过快照读裸配置 |

Config 自己持有唯一 active `{ProjectConfigLocation, ConfigSnapshot}`。启动 / resume 的协调 ACL 把 Project-owned `ProjectIdentity` 映射成 Config-owned `ProjectConfigLocation` 后调用 Config participant；Config **NEVER** import Project PL，因此 `Config → Project` 的全局上游方向不会形成物理循环。每个 Main Run 在 shared session lease 下捕获一次 active snapshot；Provider / Tool / Hook / Policy / Reflection factory **NEVER** 回读进程级 current 配置。同步 `ConfigReader` 只是 Config-internal committed-state view，只能被 coordinator / gate-aware façade 持有；AgentClient application implementation 只消费 async `ConfigQuery` / `ConfigWriter`，非 Run query / subscribe 先取得 shared session-switch permit。TUI / CLI 只经 Runtime-owned AgentClient command 与 SDK event 投影配置，**NEVER** 直连 Config OHS、subscription 或 watch receiver。

## 6. 持久化：数据 BC → Storage

| 上游 | 下游 | 模式 | 说明 |
|---|---|---|---|
| Context Management / Memory / Audit | Storage | C/S | Storage 提供原子写 / 损坏隔离**机制**，不拥有数据本体。**Session 落盘时内嵌 Task / Project 快照**（Context Management 经 `TaskPersist` / Project-owned `WorkspacePersist` 收集）；TaskStore 是纯内存聚合，**NEVER** 另建 Task → Storage 路径。恢复时 Context Management 先取得同一排他 gate，再在 gate 内依次 prepare Project → Config → Memory → Task，执行无失败 commit 并发布 Session / active resources 后才释放。Project 也不单独持久化同一份 Workspace Snapshot。Memory 独立持久化 project memory；Audit 通过 `AppendLogPort` 持久化 Usage facts。Tool Result blob 由 Tool/Context Management 的窄端口按需写入 Storage。 |

## 7. Shared Kernel（谨慎，尽量小）

| 共享内核 | 参与 BC | 风险控制 |
|---|---|---|
| `Message`（对话消息类型） | Agent Runtime / Context Management / Provider | 最小化，只放稳定核心类型 |
| `ReasoningLevel`（`Off / Low / Medium / High / Xhigh / Max`） | Workflow / Config / Agent Runtime / Context Management / Provider | 只共享稳定有序枚举；graph、user max、model capability 与 wire 映射仍归各 BC 所有 |
| `Task` 类型 | 实为 **Task BC 的 Published Language**（非 SK），其他 BC 引用其发布类型 | 不变量由 Task BC 独占 |

领域标识 **NEVER** 使用“全域 ID”共享内核：RunId / ToolCallId 等 UUIDv7 newtype 由各自所有者发布，TaskId / BatchId 使用 Task-owned 单 Session 数字格式，WorkspaceId 使用 Project-owned deterministic opaque 格式。跨 BC 只消费所有者发布的精确类型，**NEVER** 退化为无所有权的通用 `Id`。

## 8. 关键 ACL 位置（防腐重点）

1. **Provider 内部**：各家 LLM API → 统一领域调用与 `Message`（最重的 ACL）。
2. **TUI ACL**：SDK `ChatEvent::WorkingDirectoryChanged { workspace: WorkspaceContextView, .. }` 先转换为 TUI-owned `WorkspaceSnapshot`，再由 `AgentEventMapper` 生成 Intent；Model **NEVER** 持有 SDK / Project 类型。branch / worktree kind 经异步 Effect 回填。
3. **Session 快照组装与恢复**：Context Management 落盘时经 `TaskPersist` / `WorkspacePersist` 收快照；恢复时先取得排他 gate，再在同一 gate 内联合 prepare + 无失败 commit + 发布 Session / Project identity，最后释放——经窄端口，不共享内部结构。

## 9. 未来演进的地图预留

| 演进 | 版本 | 地图影响 |
|---|---|---|
| **Server 化** | v0.1.0 之后 | Server 作为入站适配器接 `AgentClient`；**核心域对传输层透明**（进程内直调 / WS 远程二选一，核心不改）。 |
| **单 main + 多 sub** | v0.2.0 | 由 Agent Runtime 的 SubAgent 能力承担（多个子 Run），**不属编排**，不新增 BC。无多-agent 图编排的长期计划。 |

## 10. 四条 Context Map 决策

1. **Audit = Pub/Sub 单向 Usage 事实**：Runtime 只做非阻塞 try_record，不等待 Audit IO；Audit MVP 只记录 Usage metadata，不影响 Runtime。Cost/Pricing 保留为 Future。
2. **Interaction 不成 BC**：ask_user / 权限审批 / plan mode / pause-resume 是 Runtime 的用例族，经唯一 Runtime-owned `InteractionPort` + Policy-owned `PolicyPort` 协作，由不同触发源（tool suspension / policy / user）复用。
3. **Task 类型 = Task BC 的 Published Language**（非 Shared Kernel）：由 Task BC 独占不变量，其他 BC 引用其发布类型。
4. **Runtime 无 Project 端口**：`WorkspaceMode` 只驱动 Composition 的 scope 策略；Main 复用 active-main-session-slot 的同一 Project wiring，Sub 才从父 scope 派生 isolated run scope。Tool 与 Context Management 直接消费对应 wiring 发布的窄 view，**NEVER** 再叠加 Runtime 或 Tool Workspace façade。

## 11. 相关文档

- 产品与子域：[01-product-and-domain.md](01-product-and-domain.md)
- 统一语言：[02-ubiquitous-language.md](02-ubiquitous-language.md)
- 系统架构与六边形：[04-system-architecture.md](04-system-architecture.md)
- 依赖规则：[05-dependency-rules.md](05-dependency-rules.md)
- 代码组织规范：[06-code-organization.md](06-code-organization.md)
- Project Workspace 端口：[../02-modules/project/02-ports-and-adapters.md](../02-modules/project/02-ports-and-adapters.md)
- 迁移治理：[../03-engineering/migration-governance.md](../03-engineering/migration-governance.md)
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
| 2026-07-12 | 补齐 Audit/Tool Result → Storage 机制边，并明确 Project Snapshot 由 Session 组装而非重复落盘 | #793 |
| 2026-07-12 | 补充 Runtime Application Control → Application Version Control 出站边界 | #793 |
| 2026-07-12 | Policy 收缩为 AllowAll-only 实现；Hook 单端口；Audit MVP 收缩为非阻塞 UsageSink，并明确 Audit→Storage AppendLog 语义 | #790 |
| 2026-07-14 | 移除不可闭合的 Runtime → Project 端口，改由 Composition internal Run scope 保留隔离 wiring，并补齐 Tool / Context Management 直连 Project 与 TUI ACL 投影边 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
