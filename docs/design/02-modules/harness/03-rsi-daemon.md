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
| **Timer（主）** | daemon 内定时器到达 `harness.rsi.interval_secs`（默认 28800，即 8 小时，可配置） | 与交互会话零耦合——不依赖 run 计数或空闲检测（独立进程无需会话态）；默认一天 3 轮，介于低频整理与积压消化之间 |
| Manual（辅助） | 控制面 `Trigger`（slash / CLI） | 显式即时触发；与 Timer 撞车时排队（串行化），NEVER 并行两个周期 |

## 4. 分块改进循环（daemon 内）

单个改进周期材料量大（组件树全量 + 跨会话行为投影 + 经验库），**MUST 分块执行**：

1. **材料分块**：按组件分组读入（每块 = 一个/一组组件 + 与其相关的行为投影切片 + 相关 Active lessons）；跨会话行为投影按时间窗口切块。
2. **分块提案**：LLM 逐块处理——每块独立产出局部 ChangeSet（每条 edit 附证据引用与理由，结构化输出 + JSON 兜底，对齐反思解析修复链路经验：空输出/解析失败重试与修复上限）。single-phase 兼容路径使用短 evidence ref；two-phase 则先由宿主路由候选组件，再由诊断阶段选择固定目标。
3. **分块落盘**：每块局部 ChangeSet 独立 validate → 成功即原子写盘（块级采纳）→ 回写该块经验；**单块失败只作废该块**，NEVER 拖垮整个周期（区别于交互提案的整份作废：定时批量场景块级幂等优先）。two-phase 执行只写诊断指定组件，NEVER 输出 component id/action。
4. 周期结束记录汇总（采纳/作废/无变更计数）入经验库与日志。

变更对会话的生效路径：daemon 落盘 → 活跃会话在下一 run 边界热载检测到变更 → 防御性复检 → 生效/回退 → **Remind 通知 LLM**（复用 GuidanceReloadPolicy 通道，告知组件已更新）。

**本里程碑为单轴**：不做 invariant/domain 双轴与多 rollout 并行探测（0.3.0+）。

## 5. 配置（`harness.rsi.*`，对齐 `memory.reflection.*` 范式）

| 键 | 默认 | 说明 |
|---|---|---|
| `enabled` | true | 总开关（false 时 daemon 不启动定时器） |
| `model` | 继承默认 | 独立模型档 |
| `max_tokens` / `reasoning` | 继承 catalog | 独立输出预算与思考档（结构化 JSON 输出任务需显式降思考档的教训已在反思链路验证） |
| `interval_secs` | 28800 | Timer 触发周期（8 小时；一天 3 轮） |
| `evidence_initial_window_days` | 30 | 首次（无 checkpoint）读取的活跃 session 时间窗（天） |
| `evidence_max_runs_per_cycle` | 50 | 每周期步长上限：最多处理的 run 数，超出留待下周期增量消化。默认配合 8 小时周期；按真实 corpus 模拟，session 不跨块时平均约 11–12 个数据块/轮。旧键 `evidence_max_sessions_per_cycle` 仅兼容 alias |
| `evidence_block_runs` | 5 | 数据源分块粒度：每块包含的 run 数上限；旧键 `evidence_block_sessions` 仅兼容 alias（流模型见 04 分片 §2A.5/2A.6） |
| `chunk_max_components` | 待废弃 | v1 按组件分块的遗留参数，语义降级为「单块内组件面体积兜底」（主分块单位是数据源块，04 分片 §2A.5） |
| `execution_mode` | `two_phase` | 改进执行模式：`two_phase`（默认：宿主路由候选 → 诊断选择固定组件 → 执行只写正文）/ `single_phase`（一次提案即落盘，v1 兼容路径，带路由止血校验）/ `diagnose_only`（仅诊断，不落盘） |

**内建 prose 组件 bootstrap**：`guardrails/default` 与 `tool-guidance/default` 是宿主 canonical 职责面，不属于 RSI 动态创建的任意组件。正式 `rsi run` / `rsi serve` 装配时若文件缺失，宿主原子创建最小骨架（`# Guardrails\n` / `# Tool Guidance\n`），幂等且 NEVER 覆盖用户文件；`rsi run --dry-run` 只在虚拟 registry 中列出缺失骨架并参与路由/执行预览，NEVER 写盘。`rsi history` / `rsi rollback` 保持只读装配，不触发创建。

## 6. 执行模式与职责边界（2026-10-22 拍板修订）

### 两种执行模式

| 模式 | 命令 | 行为 |
|---|---|---|
| **一次性执行** | `aemeath rsi run` | 执行单轮改进周期（分块提案→落盘）后退出；Manual 语义的 CLI 面，e2e 验收与手动触发入口 |
| **后台定时** | `aemeath rsi serve` | 常驻 daemon + Timer 周期 + UDS 控制面（Trigger/Status/Stop） |

两模式共用同一改进循环与 registry（仅调度外壳不同）。

### CLI 参数面（`rsi run`）

```
aemeath rsi run [--dry-run] [--max-runs N] [--window-days N] [--json]
```

| 参数 | 说明 |
|---|---|
| `--dry-run` | 预演模式（见下） |
| `--max-runs N` | 覆盖 `evidence_max_runs_per_cycle`（仅本次；单位是 run）。旧参数 `--max-sessions` 已移除，NEVER 作为 alias 保留 |
| `--window-days N` | 覆盖 `evidence_initial_window_days`（仅本次；仅无 checkpoint 时生效） |
| `--json` | 结果以 JSON 输出（机器可读，e2e/脚本用） |

### dry-run 模式（2026-10-23 拍板增补）

`aemeath rsi run --dry-run` 执行**完整周期但不产生任何持久副作用**：

- ✅ 执行：证据投影（按流模型取数）→ 组件路由 → LLM 诊断/提案 → ChangeSet 校验（含整份作废预检）→ **虚拟采纳** → Compose 仲裁
- **虚拟终态**：dry-run 使用内存 overlay registry。改进块与 Compose 的编辑只写入 overlay，后续块和 Compose 都能看到叠加后的虚拟终态；底层真实组件 registry NEVER 被写入
- ❌ 不执行：组件落盘、lesson 写入、checkpoint 推进——三者全部跳过（校验失败的作废同样只报告不落任何状态；Compose 修正同样只预览不落盘）
- **预览输出**：每块提案摘要与 Compose 仲裁摘要（组件 id + 编辑类型 + reason + diff 概要）写入 stdout（`--json` 时输出结构化结果），并落 `~/.agents/harness/dry-run-preview.md` 供事后查看（预览文件 ALWAYS 覆盖写，NEVER 累积）
- **checkpoint 语义**：dry-run NEVER 推进游标——下次正式跑仍从原游标取同一批 session（这正是预演的意义：看到「这批证据会产出什么提案」再决定是否正式执行）
- `rsi serve` 不提供 dry-run（定时自动跑无预览消费方）

### 职责边界：reflect NEVER 进入 harness 进程

harness daemon **保持单一职责**（纯 harness 组件治理）。memory reflect 维持主进程 detached 派发现状，PreCompact 触发保留——「reflect 迁入 daemon + PreCompact 退役」方案已取消（原 #2029，用户 2026-10-22 拍板：harness 进程不宜过重）。

TaskKind 插拔面保留（c8 plugin 等后续组件治理任务可注册），但当前仅 `HarnessRsi` 一个注册任务；`MemoryReflect` 注册位撤销。

## 7. 验收

- **daemon 生命周期**：常驻不自动退出、显式 stop、崩溃后重拉，主会话全程可用（L4 场景）。
- **控制协议**：Trigger/Status/Stop 往返、Busy 排队路径。
- **dry-run 全链路**：有虚拟采纳时 MUST 进入 Compose；Compose prompt 使用内存叠加终态与采纳前 lesson 快照；真实 registry write/delete、lesson append/retire、checkpoint 推进全部为 0（L2 + 真模型 e2e）。
- **组件路由**：signal → 候选组件、Skill 精确匹配、unknown unresolved、single-phase 路由外组件拒绝、two-phase 执行固定组件均有 L2 测试；事件流记录候选/选择/拒绝/unresolved。
- **分块**：大材料分块处理、单块失败不影响其余块（L2）。
- **真模型 e2e**：Timer 触发 → daemon 分块提案 → daemon 落盘 → 活跃会话下一 run 生效且收到 Remind 通知 → 经验库含证据记录；**无会话时离线改进路径同样验证**。mock 全绿不构成本项验收（既有教训）。
- TUI：Manual 回执可见；Timer 周期静默（结果可查）。
