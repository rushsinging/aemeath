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

开始实现前 **MUST** 逐项完成以下设计、评审与验证；**NEVER** 以本文件当前草案代码作为 Target 契约。

### 3.1 协议与 Codec

- wire codec 版本协商与向前兼容（`Call`/`Resp`/`Frame` 枚举不得在未定义 fallback 路径下做 breaking change）；
- 确认 `AgentClient` trait 已冻结（Target freeze），`set_reasoning_level`/`reply_interaction`/`cancel_interaction`/`cancel_run` 等所有方法签名与语义不再变更；
- `Call`/`Resp` 是否复用 SDK Published Language 的最终确认（不允许现有草案中的非闭合 enum 直接作为线协议 schema）；
- `packages/agent-wire` crate 的独立强边界收益论证：独立编译单元、无环依赖、可独立版本化发布（按 [代码组织规范](../../01-system/06-code-organization.md) §3.6）。

### 3.2 安全与传输

- TLS/WSS 全链路 + 证书链校验 + 双向 mTLS（视场景），禁止明文 WS 生产路径；
- token 签发/过期/吊销/scope 完整生命周期设计；
- session binding：token 与 session 强绑定，防止跨会话越权复用；
- nonce / timestamp + replay protection 防重放攻击；
- UDS peer credential 校验（`SO_PEERCRED`/`LOCAL_PEERCRED`）+ socket 文件权限收紧 + worker capability 最小权限约束；
- UDS 权限模型与 stale UDS socket 清理策略。

### 3.3 流量与资源治理

- 单帧/单连接消息大小上限；
- per-connection / per-session rate limit；
- backpressure 策略（慢消费者/慢 worker 处理、有界队列、禁止无界内存增长）；
- per-session / per-tenant 配额与资源隔离（CPU/内存/磁盘/网络）；
- worker 间公平调度。

### 3.4 会话与多客户端

- session ownership 模型：谁创建、谁可 attach、谁可销毁；
- multi-client 并发接入同一 session 的语义与冲突仲裁；
- role dispatch：不同角色（owner/observer/contributor）的帧能力矩阵；
- environment allowlist：worker 可访问的 shell command/文件系统路径/网络出口白名单。

### 3.5 边界与架构约束

- 控制面 **NEVER** 直接依赖 Storage BC 或 Composition 业务——worker **MUST** 自行 `composition::build_agent_client()` + `load_session()`，控制面不参与 domain 对象构建或存储读写；
- `session_id` **不是认证凭据**——仅用于路由/会话定位，鉴权/授权必须基于独立 token/credential，不得以持有 `session_id` 即视为已认证；
- `AgentId`、Run 生命周期与 Runtime Target 对齐确认。

### 3.6 代码草图清理

当前 01-design.md 中出现的 `Call`/`Resp`/`Frame`/`WsConn`/`pipe_bidirectional` 等 Rust 代码片段 **全部为非规范研究 sketch**——enum 未闭合、trait 未冻结、错误类型未定义、边界语义未指定。它们在正式设计门禁全部通过前 **不得被视为 Target 契约，不得作为实现基线**。正式设计 **MAY** 选择完全不同的抽象或放弃这些草图，不承担兼容义务。

## 4. Future 设计边界

正式设计 **MUST** 保持以下边界；若需改变，必须先更新系统级 Context Map：

- **MUST** 与 TUI 同级，共享 `AgentClient` 端口契约，不引入第二套入站接口。
- **MUST** 控制面保持薄——只做路由 / 调度 / 隔离 / 代理，**NEVER** 承载领域实体或业务规则。
- **MUST** `Call`/`Resp` 是唯一一套协议，控制面帧内容一律透传，不反序列化。
- **MUST** Worker 侧 Runtime 核心一行不改，只加 WS server 适配层。
- **NEVER** 将 Domain 聚合直接暴露给远端客户端。
- **NEVER** 控制面直接依赖 Storage BC 或 Composition 业务——worker **MUST** 自行 `composition::build_agent_client()` + `load_session()`，控制面不参与 domain 对象的构建或存储读写。
- **NEVER** 将 `session_id` 作为认证凭据——`session_id` 仅用于路由/会话定位，鉴权/授权必须基于独立 token/credential，不得以持有 `session_id` 即视为已认证。

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
