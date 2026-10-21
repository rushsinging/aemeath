# Harness · RSI 独立进程引擎

> 层级：02-modules / harness｜状态：Target｜Milestone：v0.2.0
> 本文定义递归自我改进（RSI）引擎的进程模型、通信协议、触发机制与单步改进循环。**承载为独立进程**（已拍板：memory reflect 不承担 harness 编辑职责）。

## 1. 进程模型

- **子命令**：`aemeath rsi serve`（对齐 `aemeath systemone serve` 先例），UDS socket 通信，路径按 `specs/3.9-config-compat.md` 运行时路径约定落在 `~/.agents/` 下（实施时定具体文件名）。
- **惰性拉起**：主进程在触发条件满足时 spawn daemon；daemon 空闲超过 TTL 自动退出（对齐 scoring daemon 生命周期先例）。
- **崩溃隔离**：daemon 崩溃/超时/不可达时主会话**不受任何影响**——触发请求静默降级 + warn 日志；下一触发周期重新拉起。NEVER 因 RSI 失败阻塞用户 turn。
- **单一职责**：daemon 内运行 LLM 改进循环与经验库读写；组件写盘由**主进程**执行（daemon 只产出 ChangeSet），保证 validate/原子写/热载都在主进程侧闭环。

## 2. 通信协议（UDS，帧式 JSON）

```rust
// 主进程 → daemon
struct RsiRequest {
    request_id: String,
    trigger: RsiTrigger,           // Manual | Interval | Idle
    materials: MaterialSnapshot,   // 见下
}
// daemon → 主进程
enum RsiResponse {
    ChangeSet(ChangeSet),          // 采纳提案（omitted-is-inherited）
    NoChange { reason: String },   // 证据不支持任何变更（合法结果，非失败）
    Failed { error: String },      // 解析/模型失败
}
```

**MaterialSnapshot**（输入材料，主进程组装）：

| 字段 | 内容 |
|---|---|
| component_tree | 经 registry `read` 的当前组件内容（参与编辑面的组件） |
| behavior_signals | 行为对比信号汇总（见 04 分片）：token 效率、重试/纠错、用户反馈事件 |
| recent_runs | 最近 N 个 run 的摘要（目标、工具使用、结果状态） |
| library_digest | 经验库摘要（daemon 自己也可读全库，此处传轻量摘要供主进程日志） |

**ChangeSet** 即 01 分片编辑契约；主进程收到后：统一 validate → 失败整份作废并回告 daemon；成功则统一 write，下一 run 边界热载生效。

## 3. 触发机制

| 触发 | 条件 | 说明 |
|---|---|---|
| Manual | slash 命令显式请求 | 用户即时发起，完成后 TUI 回执（采纳/无变更/失败） |
| Interval | 会话 run 计数达到 `harness.rsi.interval_runs`（默认 10） | 计数只对 Conversation run 递增（对齐 reflect 频控口径） |
| Idle | 空闲窗口达到 `harness.rsi.idle_secs` | 复用 wakeup 机制派发，daemon 完成后静默 |

同一时间仅一个改进任务在跑（daemon 侧串行化）；触发撞车时 Interval/Idle 让位 Manual。

## 4. 单步改进循环（daemon 内）

1. 读材料：组件树 + 行为信号 + 经验库（改进级递归输入——历史 lesson 指导本次提案）。
2. 提案：LLM 产出 ChangeSet（每条 edit 附证据引用与理由，结构化输出 + JSON 兜底，对齐反思解析修复链路经验：空输出/解析失败重试与修复上限）。
3. 自校验：daemon 侧预检（正文长度、frontmatter 形状等无模型成本检查）。
4. 返回 ChangeSet 由主进程终审执行。
5. 结果回写经验库（采纳/作废 + 证据绑定，见 04 分片）。

**本里程碑为单轴单步**：不做 invariant/domain 双轴与多 rollout 并行探测（0.3.0+）。

## 5. 配置（`harness.rsi.*`，对齐 `memory.reflection.*` 范式）

| 键 | 默认 | 说明 |
|---|---|---|
| `enabled` | true | 总开关 |
| `model` | 继承默认 | 独立模型档 |
| `max_tokens` / `reasoning` | 继承 catalog | 独立输出预算与思考档（结构化 JSON 输出任务需显式降思考档的教训已在反思链路验证） |
| `interval_runs` | 10 | Interval 触发阈值 |
| `idle_secs` | 600 | Idle 触发阈值 |

## 6. 验收

- **daemon 生命周期**：拉起/空闲退出/崩溃后重拉，主会话全程可用（L4 场景）。
- **协议**：请求-响应往返、超时降级、NoChange 路径。
- **真模型 e2e**：触发 → daemon 产出 ChangeSet → 主进程 validate/write → 下一 run 组件新版本生效 → 经验库含证据记录。mock 全绿不构成本项验收（既有教训）。
- TUI：Manual 回执可见；Interval/Idle 静默。
