# Server（通用域 · 入站适配器）

> 层级：02-modules / server（模块战术设计）
> 状态：**⏸ Deferred — 暂不设计**｜Milestone：v0.1.0 之后
> 对应 Issue：#794（S2，暂缓）｜伞 Issue：#743
> Server 是远端客户端接入 Agent Runtime 的入站适配器，与 TUI 同级共享 `AgentClient` 端口契约。战术设计暂缓至实际需要时再启动。

## 1. 为什么暂缓

Server Foundation 属于 **v0.1.0 之后**的能力，当前里程碑（Context Engineering + 架构重构）不落地 Server 代码。伞 issue #743 的 S2 全模块设计中，Server 是唯一纯远期模块——没有运行时代码、没有消费方依赖、不阻塞 S3–S7 任一阶段。

在 Runtime 核心（#761）、Context Management（#786）、Tool（#787）、Provider（#788）、Workflow + Config（#792）等模块战术设计已定型的前提下，Server 设计可以安全推迟，不会引起上游端口返工。

## 2. 已有草案

`docs/design/07-server-design.md` 已包含完整设计草案，涵盖：

- 六边形端口定位（入站适配器，与 TUI 同级）
- 进程拓扑（控制面 + worker，单一 `aemeath` 二进制三种角色）
- `Call`/`Resp`/`Frame` 协议（复用 SDK Published Language，控制面透传不解析）
- Worker 侧（runtime 不改，加 WS server）
- 控制面侧（WsProxy 双向透传 + SessionManager 路由）
- CLI 双模式（`--server` flag）
- 多 Agent 维度（`AgentId` 预留，Single 模式退化无感）
- 失败处理与存储 MVP 策略
- 新增 crate 规划（`packages/agent-wire`、`apps/server`）

待正式启动时，以此草案为基础，按其他模块（如 provider/、tools/）的战术设计深度拆分为多份文档。

## 3. 启动条件

Server 战术设计在以下任一条件满足时启动：

- v0.1.0 发布后进入 Server Foundation MVP 开发里程碑
- 上游端口（`AgentClient`、SDK Published Language）发生 breaking change，需要同步评估 Server 影响

启动时需重新确认：

- `Call`/`Resp` 协议是否仍复用现有 SDK 类型
- `AgentId` 多 agent 维度是否与 Runtime S3（#700）Run 模型对齐
- Worker 进程隔离策略是否与恢复语义（#762）一致

## 4. 设计边界（草案已确认的约束）

以下约束在草案中已明确，正式设计时 **MUST** 继承：

- **MUST** 与 TUI 同级，共享 `AgentClient` 端口契约，不引入第二套入站接口。
- **MUST** 控制面保持薄——只做路由 / 调度 / 隔离 / 代理，**NEVER** 承载领域实体或业务规则。
- **MUST** `Call`/`Resp` 是唯一一套协议，控制面帧内容一律透传，不反序列化。
- **MUST** Worker 侧 Runtime 核心一行不改，只加 WS server 适配层。
- **NEVER** 将 Domain 聚合直接暴露给远端客户端。

## 5. 相关文档

- 设计草案：[../../07-server-design.md](../../07-server-design.md)
- 系统总体设计：[../../01-system/](../../01-system/)
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：占位文档，标注暂缓设计，继承草案约束 | #794 |
