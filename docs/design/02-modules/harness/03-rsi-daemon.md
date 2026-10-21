# Harness · RSI 独立进程引擎

> 层级：02-modules / harness｜状态：Target｜Milestone：v0.2.0
> 本文定义递归自我改进（RSI）引擎的进程模型、控制协议、定时触发、分块改进循环与落盘职责。**承载为常驻独立进程**（已拍板：memory reflect 不承担 harness 编辑职责；daemon 负责落盘）。

## 1. 进程模型

- **子命令**：`aemeath rsi serve`（对齐 `aemeath systemone serve` 先例），UDS socket 控制面路径按 `specs/3.9-config-compat.md` 运行时路径约定落在 `~/.agents/` 下（实施时定具体文件名）。
- **常驻**：daemon 启动后**NEVER 空闲自动退出**；停止只能经控制面显式 `stop`（或进程级终止）。启动方式：用户/服务管理器手动启动，或首次 Manual 触发时拉起——拉起后常驻。
- **崩溃隔离**：daemon 崩溃不影响任何主会话；下一次定时周期或 Manual 触发时重新拉起。主进程对 daemon 的依赖仅为可选控制面，NEVER 因 RSI 失败阻塞用户 turn。
- **落盘职责**：daemon 持有 registry **完整读写路径**——validate + 原子写盘（tmp+rename）都在 daemon 侧完成。主进程只做 run 边界热载检测（detect + 防御性复检）与 Remind 通知（见 02 分片）。原子 rename 保证 daemon 写与主进程读永不读到半写状态：读方看到的一定是完整旧版或完整新版，无需跨进程锁。

## 2. 控制协议（UDS，帧式 JSON）——仅控制面

daemon 自主定时运行，主进程不需要「请求改进」；UDS 仅承载控制指令：

```rust
// 控制方（主进程 slash / CLI）→ daemon
enum RsiControl {
    Trigger { request_id: String },   // Manual 显式触发一轮改进周期
    Status,                            // 运行状态查询（当前周期/上次结果）
    Stop,                              // 显式停止
}
// daemon → 控制方
enum RsiControlAck {
    Started { request_id: String },    // 周期已开始（结果异步，经日志与经验库可查）
    Status(RsiRunState),
    Stopped,
    Busy,                              // 已有周期在跑，Manual 让位
}
```

**数据面全走落盘**：daemon 的输入材料（组件文件、会话行为投影、经验库）与输出（变更落盘、经验库回写）全部是 `~/.agents/` 下的文件，与主进程零数据面耦合——**会话全部关闭后 daemon 仍可离线改进**。

## 3. 触发机制

| 触发 | 条件 | 说明 |
|---|---|---|
| **Timer（主）** | daemon 内定时器到达 `harness.rsi.interval_secs`（默认 1800，可配置） | 与交互会话零耦合——不依赖 run 计数或空闲检测（独立进程无需会话态） |
| Manual（辅助） | 控制面 `Trigger`（slash / CLI） | 显式即时触发；与 Timer 撞车时排队（串行化），NEVER 并行两个周期 |

## 4. 分块改进循环（daemon 内）

单个改进周期材料量大（组件树全量 + 跨会话行为投影 + 经验库），**MUST 分块执行**：

1. **材料分块**：按组件分组读入（每块 = 一个/一组组件 + 与其相关的行为投影切片 + 相关 Active lessons）；跨会话行为投影按时间窗口切块。
2. **分块提案**：LLM 逐块处理——每块独立产出局部 ChangeSet（每条 edit 附证据引用与理由，结构化输出 + JSON 兜底，对齐反思解析修复链路经验：空输出/解析失败重试与修复上限）。
3. **分块落盘**：每块局部 ChangeSet 独立 validate → 成功即原子写盘（块级采纳）→ 回写该块经验；**单块失败只作废该块**，NEVER 拖垮整个周期（区别于交互提案的整份作废：定时批量场景块级幂等优先）。
4. 周期结束记录汇总（采纳/作废/无变更计数）入经验库与日志。

变更对会话的生效路径：daemon 落盘 → 活跃会话在下一 run 边界热载检测到变更 → 防御性复检 → 生效/回退 → **Remind 通知 LLM**（复用 GuidanceReloadPolicy 通道，告知组件已更新）。

**本里程碑为单轴**：不做 invariant/domain 双轴与多 rollout 并行探测（0.3.0+）。

## 5. 配置（`harness.rsi.*`，对齐 `memory.reflection.*` 范式）

| 键 | 默认 | 说明 |
|---|---|---|
| `enabled` | true | 总开关（false 时 daemon 不启动定时器） |
| `model` | 继承默认 | 独立模型档 |
| `max_tokens` / `reasoning` | 继承 catalog | 独立输出预算与思考档（结构化 JSON 输出任务需显式降思考档的教训已在反思链路验证） |
| `interval_secs` | 1800 | Timer 触发周期 |
| `chunk_max_components` | 实施定值 | 每块材料组件数上限 |

## 6. 验收

- **daemon 生命周期**：常驻不自动退出、显式 stop、崩溃后重拉，主会话全程可用（L4 场景）。
- **控制协议**：Trigger/Status/Stop 往返、Busy 排队路径。
- **分块**：大材料分块处理、单块失败不影响其余块（L2）。
- **真模型 e2e**：Timer 触发 → daemon 分块提案 → daemon 落盘 → 活跃会话下一 run 生效且收到 Remind 通知 → 经验库含证据记录；**无会话时离线改进路径同样验证**。mock 全绿不构成本项验收（既有教训）。
- TUI：Manual 回执可见；Timer 周期静默（结果可查）。
