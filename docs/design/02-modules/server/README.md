# Server（Future · 入站适配器边界）

> 层级：02-modules / server（模块战术设计）
> 状态：Future Boundary Decision｜不属于 v0.1.0 Target
> 对应 Issue：#794｜父 Issue：#743
> Server 是远端客户端接入 Agent Runtime 的入站适配器，与 TUI 同级共享 `AgentClient` 端口契约。本文只固定 Future 边界和正式设计门禁，**NEVER** 把草案当作已批准的实现规范。

## 1. 范围决定

Server Foundation 是 **v0.1.0 Out of scope** 的 Future 能力。v0.1.0 的 Runtime / SDK 设计 **MUST** 保持可由本地 TUI 消费的 `AgentClient` OHS，但 **NEVER** 为尚未批准的远端拓扑、协议或部署形态预建 Server 类型。

未来启动 Server 交付前，#794 **MUST** 完成独立战术设计、威胁建模与依赖边界评审；任何实现 **NEVER** 仅凭本页或草案开工。

## 2. 非规范性设计输入

[`01-design.md`](01-design.md) 是正式设计时的输入，涵盖：

- 六边形端口定位（入站适配器，与 TUI 同级）
- 进程拓扑（控制面 + worker，单一 `aemeath` 二进制三种角色）
- `Call`/`Resp`/`Frame` 协议（复用 SDK Published Language，控制面透传不解析）
- Worker 侧（runtime 不改，加 WS server）
- 控制面侧（WsProxy 双向透传 + SessionManager 路由）
- CLI 双模式（`--server` flag）
- 多 Agent 维度（`AgentId` 预留，Single 模式退化无感）
- 失败处理与存储 MVP 策略
- 候选 crate 边界（`packages/agent-wire`、`apps/server`）

这些内容 **MAY** 被正式设计修改或拒绝；只有经 #794 审批并写入 Target 文档的部分才成为约束。

## 3. 正式设计门禁

开始实现前 **MUST**：

- 为 #794 关联明确 milestone 与 release branch；
- 重新确认 `Call` / `Resp` 是否复用 SDK Published Language；
- 让 `AgentId`、Run 生命周期与 Runtime Target 对齐；
- 定义认证、授权、租户隔离、重放保护、限流、断连与恢复语义；
- 按 [代码组织规范](../../01-system/06-code-organization.md) §3.6 证明任何新增 crate 的强边界收益。

## 4. Future 设计边界

正式设计 **MUST** 保持以下边界；若需改变，必须先更新系统级 Context Map：

- **MUST** 与 TUI 同级，共享 `AgentClient` 端口契约，不引入第二套入站接口。
- **MUST** 控制面保持薄——只做路由 / 调度 / 隔离 / 代理，**NEVER** 承载领域实体或业务规则。
- **MUST** `Call`/`Resp` 是唯一一套协议，控制面帧内容一律透传，不反序列化。
- **MUST** Worker 侧 Runtime 核心一行不改，只加 WS server 适配层。
- **NEVER** 将 Domain 聚合直接暴露给远端客户端。

## 5. 相关文档

- 设计草案：[./01-design.md](./01-design.md)
- 系统总体设计：[../../01-system/](../../01-system/)
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：记录 Future Server 边界与草案输入 | #794 |
| 2026-07-14 | 将进度型占位改为 Future boundary decision，并增加正式设计 / 安全 / crate 门禁 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
