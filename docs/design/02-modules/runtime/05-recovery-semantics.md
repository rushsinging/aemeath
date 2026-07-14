# Agent Runtime · 恢复语义

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2，重定义自原 Durable Model Invocation）
> 本文定义 Agent Runtime 的崩溃恢复语义：**从头开始**，不做引擎级 durable checkpoint。这是对原 #762「Durable Model Invocation」的收敛。

## 1. 核心原则：从头开始

> **崩溃后不恢复 Run 的中间状态；用户重新发起，Loop 从头执行。**

aemeath 是**人在环的交互式 CLI**，不是无人值守 workflow。崩溃后：
1. 由**用户手动重新发起**（不是引擎自动恢复）
2. 重新发起时 LLM 看到的是**真实的当前文件系统状态**（已被改过的），基于新事实重新决策
3. 副作用一致性由**「人 + 文件系统真实状态」**天然兜底——不需要引擎级 durable checkpoint

## 2. 持久化边界：什么落盘 / 什么不落盘

| 对象 | Target 持久化策略 | 说明 |
|---|---|---|
| **Session 对话历史**（ChatChain/ChatSegment）| RunStep 级快照 | 每个 RunStep 结束后落盘一次，供 `/resume` 恢复对话上下文 |
| **Task 快照 / Workspace 上下文** | 内嵌 Session | 随 Session 落盘（跨 BC 快照组装，见 `../context-management/01-session.md`）|
| **Run 聚合 / RunStatus 状态机** | 仅内存 | 崩溃即丢，从头开始 |
| **RunStep 中间状态 / ToolCall 进行态** | 仅内存 | 不做 checkpoint |
| **RuntimeContext（活资源）** | 仅运行时装配 | 崩溃后重新装配 |
| **StuckGuard 计数** | 仅内存 | 重置 |

**关键**：落盘的是**对话历史数据**（Context Management 的 Session），**不是** Run 的执行状态机。这与"单状态机内存态"完全自洽——持久化的是数据，不是状态机。

## 3. 崩溃恢复流程

```
崩溃 → 进程重启 → 用户 /resume 或重新输入
  │
  ▼
Context Management 加载最近 Session 快照（对话历史 + Task/Workspace 快照分发回各 BC）
  │
  ▼
用户输入 → agent_run 新建 Run（全新 RunId，Created 态）
  │
  ▼
Loop Engine 从头跑：LLM 基于「已落盘对话历史 + 真实当前文件系统」重新决策
```

- **不重放**未完成的 Run Step / ToolCall
- **不恢复** AwaitingUser 暂停点（崩溃时若在 ask_user，重来时用户重新表达）

## 4. 副作用一致性（为什么"从头"是安全的）

设想崩溃前 Run 已 `Write` 3 个文件、跑 2 个 `Bash`，第 4 步崩溃：

- **从头重发同样输入**，LLM 看到的是**已被改过的文件系统**（3 个文件已存在），不会机械重放
- LLM 基于真实新状态重新决策——可能跳过已完成的、或基于现状继续
- **人是最终裁判**：交互式 CLI 下用户实时观察，异常时可干预

> 对比无人值守 workflow：那里没有人兜底，才需要 durable checkpoint 防重复副作用。aemeath 有人，所以不需要。

## 5. Sub Run 的恢复

- **Sub Run 不落盘**
- 父 Run 崩溃 → 从头开始 → 若父重新决策仍需该 sub，则**重新派生**子 Run
- Sub 的副作用（文件/命令）同样由父重新决策 + 文件系统真实状态兜底

## 6. 明确不做（对原 #762 的收敛）

- **NEVER** 建立 Durable Model Invocation checkpoint 链
- **NEVER** 在每次 LLM 调用前后持久化 Run 状态
- **NEVER** 为 Run 引入 revision / 单调版本号 checkpoint
- **NEVER** 持久化 partial stream
- **NEVER** 引入 RecoveryRequired / fail-closed Run 恢复状态

**保留**：RunStep 级 Session 快照——这是"对话历史可 resume"，不是"执行状态可恢复"。

## 7. 相关文档

- 领域模型（Run 内存态）：[01-domain-model.md](01-domain-model.md)
- 状态机（AwaitingUser 不落盘）：[03-loop-and-state-machine.md](03-loop-and-state-machine.md)
- Session 聚合与快照：[../context-management/01-session.md](../context-management/01-session.md)
- 依赖规则（无 durable 原则）：[../../01-system/05-dependency-rules.md](../../01-system/05-dependency-rules.md)
- Current → Target 迁移责任：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：从头开始恢复语义、持久化边界、副作用一致性、Sub 恢复、明确不做 durable（收敛原 #762）| #761 |
| 2026-07-11 | agent_execution→agent_run | #761 |
