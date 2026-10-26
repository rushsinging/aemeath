# Harness · 证据反思与经验库

> 层级：02-modules / harness｜状态：Target｜Milestone：v0.2.0
> 本文定义无 ground-truth verifier 场景下的 RSI 效果反思：行为对比信号、证据绑定与经验库 corpus。

## 1. 设计立场

交互式会话没有 benchmark 评分。效果判断依赖**行为对比信号**：改进前后同类任务的行为差异。信号只作为 LLM 提案与采纳判断的**证据输入**，NEVER 用阈值规则机械自动采纳（对齐「语义合并判断交给 LLM，系统负责门禁」的反思职责边界先例）。

## 2. 信号集（daemon 从落盘投影读取，分块进材料）

| 信号 | 来源 | 说明 |
|---|---|---|
| token 效率 | run usage 汇总 | 每 run 输入/输出消耗与预算比 |
| 重试与纠错 | 工具事件流 | Bash 失败重跑、Edit 冲突重试、恢复性命令 |
| 用户反馈 | 交互事件 | 确认（accept）、纠正（reject 后改写重发）、打断 |
| 任务结局 | run 终态 | 完成/失败/超时/取消 |

信号为**既有落盘事件的汇总视图**（runtime 事件流与 usage 事实的落盘投影），NEVER 新建独立埋点通道，也 NEVER 依赖主进程内存态——会话全部关闭后 daemon 仍可离线读取。

## 2A. 执行证据装配（SessionEvidenceProjection，2026-10-23 增补）

> 背景：v1 实施后复盘发现材料只有「组件内容 + lessons」，缺执行证据——改进依据是组件长什么样而非 agent 实际怎么跑，模型只能发现表面缺陷（TODO 空壳）。对照 Harness² 源码（`core/prompts.py` propose）：**每条编辑 MUST 追溯到轨迹中的某个时刻，否则就是投机**（"If you cannot name the moment, the edit is speculation"）。本节把 §2 的信号集落为可实施的投影装配。

### 2A.1 数据源（全部既有落盘，NEVER 新埋点）

| 来源 | 位置 | 提供的信号 |
|---|---|---|
| step 记录 | `~/.agents/session/<id>.dataset/members/<hash>`（manifest 登记成员） | 消息区含 ToolResult 原文（错误文本、重试、超长触发）、accepted_input |
| session 状态 | 同 dataset 的 `session-state` 成员 | chats、compact 标记、committed_steps 数 |
| session 元数据 | 同 dataset 的 `metadata` 成员 | 模型、创建/更新时间、reasoning 档 |

投影器只读 dataset（manifest 定位成员、按成员清单流式扫描），NEVER 依赖主进程内存态。

### 2A.2 投影器（`SessionEvidenceProjector`，harness crate）

- **输入**：sessions 根目录 + 证据流窗口（由 2A.5 流模型决定：checkpoint 游标 + 步长 + 首次 30 天窗口；投影器只处理本次周期分到的 session 段）。
- **输出**：`EvidenceBundle`（分块进材料，见 2A.3），聚合而非全文——**NEVER 把 ToolResult 原文整段进材料**，每信号类最多携带 2 条有界摘录（对齐 memory 内容预览常量族）。

聚合信号类（对齐 §2 信号集 + Harness² 三级优先）：

| 优先级 | 信号类 | 聚合逻辑 |
|---|---|---|
| 1 | `repeated_failure` | 同 (tool, 错误签名) 聚合计数 ≥2；携带计数 + 摘录 |
| 2 | `retry_pattern` | 同工具近似参数指纹的重复调用（Edit old_string 不匹配重试等） |
| 3 | `oversized_result` | 触发 materializer 落盘阈值的 tool call 统计（次数 + 体积分布） |
| 4 | `run_outcome` | run 终态分布（完成/取消/超时/ContextTooLong） |
| 5 | `user_correction` | reject 后改写重发的交互事件（有则记录） |
| 6 | `token_efficiency` | 每 run 输入/输出消耗汇总（均值与离群） |

### 2A.3 材料注入与提案门禁

- `ImprovementMaterials` 增第三字段 `evidence: EvidenceBundle`；`build_block_prompt` 在组件块与 lessons 块之间注入 **evidence 块**（证据是改进依据，lessons 是历史经验）。
- 证据块字符预算独立（`EVIDENCE_BLOCK_MAX_CHARS`，实施定值，量级 6000）；超限按 2A.2 优先级从低往高裁剪。
- **提案门禁分阶段**：v1 prompt 明文约束「每条编辑 MUST 指出它回应 evidence 中的哪条信号（引用 signal id）；无法对应的编辑是投机，NEVER 提出」；v2（后续）解析侧校验引用存在性，缺失引用的 edit 拒绝。
- 空证据兜底：窗口内无 session/无信号时渲染「（近期无执行证据）」，周期照常执行（对齐空 corpus JSON 兜底先例）。

### 2A.4 lesson 证据信号扩展

`EvidenceBinding.signal` 从单一 `cycle_observation` 扩为 `SignalKind` 枚举：

```rust
enum SignalKind {
    CycleObservation,   // v1：模型自述提案理由（保留，降为兜底信号）
    RepeatedFailure,    // 真实行为信号（2A.2 聚合）
    RetryPattern,
    OversizedResult,
    RunOutcome,
    UserCorrection,
    TokenEfficiency,
}
```

`run_ref` 扩展承载真实 `run_id`（session dataset 的 run 标识）；采纳回写时优先绑定提案引用的真实信号，`CycleObservation` 仅在无引用时兜底。**归因纪律对齐 Harness² experience reflector**：无法归因到单一信号的编辑标 `confounded` 语义（lesson 文本中显式声明），NEVER 编造归因（"an invented attribution poisons the library"）。

### 2A.4b 验证阶段（#2080）

改进采纳后不再直接「标 active 就算成功」——每周期开头用**本周期证据**判定上轮改进是否生效：

| 判定 | 条件 | 后果 |
|---|---|---|
| `verified` | 目标信号在本周期证据中消失 | 规则生效，lesson 保留 Active |
| `ineffective` | 目标信号仍出现 | 规则无效——作为重修输入（#2092 强制重修路径） |
| `pending` | 无新证据（样本不足） | 保持待验证，**NEVER 编造结论** |

- **判定依据**：lesson 落盘时绑定 `verification.target_signal`（从提案 reason 推断信号类别 → 在当轮证据中取同类完整 signal id；模型未引用时退回证据中优先级最高的信号，保证 lesson 总能进入验证）
- **信号匹配**：精确或互相包含（错误文本常被截断，严格相等会漏判）
- **幂等**：`checked_at != 0` 的条目不再判定（同一 lesson 只验证一次）
- **落盘**：`LessonEntry.verification { status, target_signal, checked_at }`；旧 JSONL 缺字段按默认 Pending/空 target 兼容
- **事件流**：`lesson_verified { lesson_id, component, target_signal, status }`
- **dry-run**：只预演判定（`judge_verification` 纯函数），NEVER 改写经验库；报告标注「预演，未落盘」

### 2A.4c 证据强度门禁与 lesson review（#2092）

**问题**：单块内 `×2` 即可落盘（偶然重试写永久规则）；同一问题分散在多个周期各 1 次则永远被过滤（永不聚合）。

**强度分档**（`StrengthThresholds`，默认值）：

| 档位 | 条件 | 处置 |
|---|---|---|
| `observe` | 累计 < 3 次 | 只报告，**不进提案材料** |
| `light` | 累计 3–5 次 | 轻量落盘（一条规则） |
| `strong` | 累计 ≥ 6 次 **或** 跨 ≥ 3 周期 | 强落盘（要求更具体的规则） |
| `rework` | 曾修过且上次验证 ineffective | 强制重修（升级强度，不重复写同类） |

**跨周期累计**（`SignalHistory` → `harness/signal-history.json`）：
- 同周期内同 signal_id 取**最大** occurrences（多块各自聚合同一信号时，重复相加会虚增强度——实测缺陷）
- 跨周期才累加班次；`last_cycle_id` 保证同周期重复调用不重复计
- 持久化原子写、fail-open（缺失/损坏 → 空历史）

**lesson review**：
- **过期清理**：距上次判定超过 `LESSON_STALE_AFTER_SECS`（30 天）且未被本周期引用 → `RetireStale`（NEVER 无界保留）
- **选择性退役**：按 review 结论退役，替代 v1 的「同组件全量退役」（有效内容不再被连带丢弃）
- **有效性复核**：复用 §2A.4b 三态状态机

**事件流**：`strength_gate { signal_id, tier, rationale }`、`lesson_reviewed { lesson_id, action, rationale }`。

### 2A.5 数据源流模型（checkpoint · 步长 · 数据源分块，2026-10-23 拍板）

> 修正 v1 语义：分块对象是**数据源（session 证据流）**，NEVER 按组件分块——组件面是每轮提案的编辑对象，不是分块单位。模型对齐 Harness² 的 continual-learning 任务流（`schedule.py::stream_order`）与断点纪律（`runmark.py`）：处理单元推进、完成显式打标、指纹校验复用。

```
session 流（按 updated_at 全序）
   │  首次：最近 initial_window_days（默认 30）天活跃 session
   │  之后：从 checkpoint 游标往后
   ▼
[步长裁剪] 每次跑最多 max_sessions_per_cycle 个 session（防单次跑爆）
   ▼
[数据源分块] 切为若干证据块（每块 = 一段 session/run 证据投影）
   ▼
每块独立：证据投影 → 全组件提案 → 采纳落盘 → lesson
   （块级幂等：单块失败只作废该块，游标 NEVER 推进过失败块）
   ▼
块成功 → checkpoint 推进（最后处理的 session id + 时间戳 + 组件树指纹）
```

**checkpoint 契约**（对齐 runmark 纪律，落盘 `~/.agents/harness/checkpoint.json`）：

1. **显式推进**：只有「块执行完成且落盘成功」才写游标——输出存在不等于完成（"presence of output is not enough"）；
2. **指纹校验**：checkpoint 记录处理时的组件树指纹；组件树在游标之后被外部修改（指纹不符）时，当块从游标重跑——NEVER 用陈旧组件状态生成的提案覆盖新改动；
3. **首次窗口**：无 checkpoint 时默认读最近 30 天（`harness.rsi.evidence_initial_window_days`，可配）活跃 session；
4. **步长上限**：`harness.rsi.evidence_max_sessions_per_cycle`——每次周期最多处理的 session 数，超出的留待下周期（增量消化，NEVER 一次吞尽）。

**组件面体积控制**：单块内若组件面过大导致材料超预算，按组件分次提案作为**体积兜底**（v1 的 `chunk_max_components` 语义降级至此并标注待废弃；正确的主分块单位永远是数据源块）。

### 2A.6 run 级游标修订（2026-10-23 拍板，修正 §2A.5 session 级缺陷）

> 复盘发现 §2A.5 的 session 级单点全序游标对「可变条目流」不完备：session 更新改 mtime 导致条目漂移——重读冗余或**永久漏读**；并发更新下 mtime 不稳定。修订为 **run 级处理状态**：

**checkpoint 形态**（`~/.agents/harness/checkpoint.json`，原子写）：

```json
{
  "sessions": {
    "<session_key>": {
      "processed_run_ids": ["run-1", "run-2"],
      "session_complete": true
    }
  }
}
```

- **增量判定**：扫描时按 step `cursor.run_id` 分组，跳过 `processed_run_ids` 集合内的 run——session 更新只新增 run，NEVER 漂移、NEVER 漏读、重读零冗余（run_id 集合判定，不依赖 mtime 可比性）；
- **步长按 run**：`evidence_max_runs_per_cycle`（每周期最多处理 run 数）；块边界优先切在 **session 边界**（尽量完成一个 session）；单 session run 数超块上限才切（`session_complete: false`，下周期续）；
- **调度序**：mtime 只决定处理顺序，NEVER 承担游标语义；
- **自动清理**：checkpoint 条目只保留 ①未完成 session（`session_complete: false`）②最近 N 条（默认 50）已完整处理的 session——超出的删除（重读风险限于极旧 session，且首次窗口天然限制选集）；盘上已消失的 session 条目即时清理。

### 2A.7 边界与排期

- Probe/Compose（多 rollout 行为对比）留 0.3.0；本节交付「单 rollout + 真实证据」。
- 投影性能：manifest 定位 + 成员流式扫描，百 run 量级毫秒级；NEVER 全量加载 ToolResult 原文进内存。
- 实施排期：harness 投影器（domain 聚合 + dataset 读取 adapter，TDD）→ improvement 材料扩展 → daemon/CLI 接线 → 真模型 e2e（验收：有证据材料下提案引用 signal id，lesson 落盘带真实 SignalKind）。作为 #2046 验收前置项。

## 3. 证据绑定（EvidenceBinding）

```rust
struct EvidenceBinding {
    run_ref: String,       // 来源 run 标识
    signal: SignalKind,    // 信号类别
    excerpt: String,       // 行为摘录（有界，预览上限对齐 memory 内容预览常量族）
}
```

**不变量**：经验库中每条被采纳的 lesson MUST 携带至少一条 EvidenceBinding；RSI 提案中每条 edit MUST 引用至少一条信号证据。无证据的提案在 daemon 自校验阶段拒绝。

## 4. 经验库 corpus

- **落盘**：独立文件（`~/.agents/harness/` 下，实施定名），JSONL 追加 + 周期整理；**NEVER** 与 memory corpus 混用路径、schema 或写锁。
- **条目**：

```rust
struct LessonEntry {
    id: LessonId,                 // UUIDv7
    component_ref: ComponentId,   // 指向哪类组件的经验
    lesson: String,               // 可复用结论（有界长度）
    evidence: Vec<EvidenceBinding>,
    adopted_at: u64,
    status: LessonStatus,         // Active | Retired（被后续经验取代）
}
```

- **复利路径**：daemon 每次改进循环读 Active lessons 作为提案上下文（改进级递归）；lesson 失效（组件删除、结论被反证）标 Retired 而非物理删除。
- **容量**：Active 上限 + 超限整理策略（实施时定数值；整理由 daemon 在改进循环内顺带完成，NEVER 独立定时任务）。

## 5. 与 memory reflect 的职责边界（MUST 遵守）

| | memory reflect | RSI 经验库 |
|---|---|---|
| 治理对象 | 记忆条目（corpus 去噪/归纳/容量收敛） | harness 组件改进经验 |
| 触发 | 独立三触发 + 写锁体系 | 本模块三触发，daemon 串行 |
| 写路径 | memory 模块独占 | harness 模块独占 |
| 进程 | 主进程 detached 派发（保持现状，reflect NEVER 进入 harness 进程——2026-10-22 拍板，见 03 分片第 6 节） | 独立 daemon |

两者 NEVER 共享 corpus、写锁或触发器；唯一交汇点是都消费 runtime 事件流的只读投影。

## 6. 验收

- 证据装配（2A）：投影器从真实 dataset fixture 提取聚合信号（L2）；材料含 evidence 块、空证据兜底渲染（L2）；真模型在有证据材料下提案引用 signal id、lesson 落盘带真实 SignalKind（e2e，#2046 前置）。
- 证据不变量：无证据提案被拒（L2）；采纳 lesson 均含 EvidenceBinding（L2 + e2e）。
- 真模型 e2e：一次完整循环后经验库出现带证据的 Active lesson，且下一次改进周期的材料分块消费到该 lesson。
- JSON 兜底：空组件树/无信号时 daemon 仍输出合法 JSON 结构（对齐反思空 corpus 兜底教训，NEVER 自然语言解释）。
