# SystemOne · 可复盘评分事件流

> 层级：02-modules / systemone（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.2.0
> 本文定义 SystemOne-owned **append-only 评分事件流**（含全量决策现场），并将既有单文件 `audit.jsonl` 升级为按日 segment。运行诊断日志（`aemeath:agent:systemone`）保持无内容原则；校准旁路 `observations.jsonl` / `calibration.json` **不在本文范围内改写**。

## 1. 定位与问题

现状双层观测：

| 层 | 路径 / target | 能力 | 缺口 |
|---|---|---|---|
| 运行诊断 | `aemeath:agent:systemone` → `~/.agents/logs/agent-systemone.log` | 失败路径 `warn`、装配诊断 | **MUST NOT** 带正文（与 Memory 同原则） |
| 可复盘事实（现状） | `~/.agents/scoring/audit.jsonl`（`AuditedScoringAdapter`） | 每次 `ScoringPort::answer` 一行：timestamp / scenario / engine_revision / prompt_sha256 / probabilities / calibration / latency / outcome | **无** state/选项正文；**无** session/run/step/`correlation_id`；单文件无限涨；无日切与保留策略 |

事后无法还原「当时看到什么、如何重排/加严」，也无法与 Memory 事件流按因果链对流。离线评测仍依赖 `eval/system-one/` harness，**不**把本事件流当作在线报告源。

因此在**评分决策发生时**必须升级事实流：

- **运行日志**：诊断定位，**MUST NOT** 携带 state / 选项正文。
- **事件流（本文）**：append-only、含全量现场、按时间窗滚动，供后期 LLM/人直接读文件复盘。

对照依据：Memory 事件流战术设计 [`../memory/06-event-stream.md`](../memory/06-event-stream.md)（同构心智；落点与内容模型按评分域裁剪）。

## 2. 已拍板决策

| 决策点 | 选择 |
|---|---|
| 与既有 `audit.jsonl` 关系 | **升级替换**：扩成含内容 + 按日切分；旧行兼容读；迁入/旁路归档后 **停止追加** 单文件 |
| 内容粒度 | **全量现场**：state 全文 + 全部选项正文 + before/after 序（或完整 probs↔选项映射）+ 完整 probabilities + 配置指纹；保留 `prompt_sha256` 对账 |
| 关联字段 | `correlation_id` + `session_id` + `run_ordinal` + `step_ordinal` + `tool_call_id?` + `scenario` + `engine_revision`；缺省显式 `null`，**NEVER** 阻断评分 |
| 保留 / GC | 默认 **30** 日历日；`scoring.event_retention_days`（`0`=禁用 GC，**NEVER**=关写入） |
| append 失败 | **fail-open**：评分主路径返回值不变；仅 `log::warn!`（无正文）到 `aemeath:agent:systemone` |
| 查询 / 报告面 | **本期不做**（无 xtask/scripts 报告、无 Scoring 查询 Port、无 LLM 工具、无 TUI/slash）；评测仍走 `eval/system-one/` |
| emit 落点 | **中心化**：`AuditedScoringAdapter`（或薄 `ScoringEventAppend`）在每次 `answer` 终态后 append 一条 |
| 校准旁路 | `observations.jsonl` / `calibration.json` **不动** |
| 内容敏感 | **明文**落本地 agents 根（与现有 scoring / memory 同信任边界）；本期无默认 redact |
| 交付 | **两 PR**（见 §9） |
| 事件落盘形态 | 真 append-only 按日 segment（jsonl） |

## 3. 非目标（本期）

- 生产查询 Port / timeline API / slash / TUI 报告面。
- 改写运行日志 schema 或为其建立消费链。
- 默认 redact、采样策略、跨机同步。
- 把事件流接入 `eval/system-one/` 作为在线验收源（harness 保持独立）。
- 改写校准观测回路（`observations.jsonl`）或温度 artifact 语义。
- 让 Storage 发布通用 append-log OHS（append 由 SystemOne adapter 自管；可复用既有 `append_jsonl_line_sync` / `SafeStorageRoot` 先例，与 Memory / Audit 同铁律）。

## 4. 落盘形态

### 4.1 事件流（目标）

- **格式**：按 UTC 日切分的 jsonl；每行一条完整 `ScoringEvent`（或演进后的 `ScoringAuditEvent`）JSON，行尾 `\n`；**NEVER** 改写已写入行。
- **路径**（相对 agents 根 / `scoring_dir`）：

```text
scoring/events/{yyyy-mm-dd}.jsonl
```

  默认绝对形态：`~/.agents/scoring/events/{yyyy-mm-dd}.jsonl`（`AEMEATH_AGENTS_DIR` 覆盖时相对该根）。
- **写入**：SystemOne-owned segment store（可内嵌于 `AuditedScoringAdapter`，或抽出 `ScoringEventAppend` + `JsonlSegmentScoringEventStore`）经 `create_dir_all` + append 追加；进程内按文件名互斥，保证同行完整。
- **保留**：默认保留最近 **30** 个日历日 segment；配置项（建议）`scoring.event_retention_days`（`0` 表示禁用 GC，**NEVER** 表示关闭事件写入）。GC 在 open / 首次 append 或低频周期触发，删除过期文件名；GC 失败只记运行日志，不阻断。
- **失败语义**：与现状一致并写死——`append` 失败 **MUST** fail-open；`ScoringPort::answer` 的 `Ok`/`Err` 原样返回。

### 4.2 旧 `audit.jsonl` 迁移

- **现状常量**：`AUDIT_FILE = "audit.jsonl"`（`agent/features/systemone/src/constants.rs`）；wiring 写入 `scoring_dir.join("audit.jsonl")`。
- **目标**：生产路径 **只** 追加到 `events/{yyyy-mm-dd}.jsonl`。
- **迁移策略（推荐默认）**：
  1. 首次启用新写路径时，若存在 legacy `scoring/audit.jsonl`：只读扫描，将可解析行 **旁路归档** 为 `scoring/events/legacy-audit.jsonl`（或按行内 timestamp 分日写入对应 segment），然后标记 / 重命名原文件为 `audit.jsonl.migrated`（或等价只读后缀），**NEVER** 再向原路径追加。
  2. 无法解析的旧行：原样抄入 legacy 归档并记 `warn`（无正文），**NEVER** 丢弃静默。
  3. 读回 helper（仅 `cfg(test)` / 测试契约）：能同时理解「新 segment + legacy 归档」；生产 **不** 提供查询 Port。
- **兼容读**：旧行缺少新字段时反序列化为缺省（`null` / 空）；新写路径 **MUST** 带 `schema_version`。

### 4.3 与校准旁路的边界

```text
~/.agents/scoring/
  calibration.json          # 温度 artifact（不动）
  observations.jsonl        # observe 回路（不动）
  events/{yyyy-mm-dd}.jsonl # 本文：可复盘评分事件
  audit.jsonl[.migrated]    # legacy，停写
```

**NEVER** 把校准观测与复盘事件流合并为同一文件。

## 5. 事件模型

### 5.1 Envelope

领域类型（示意；实现落在 `domain/event.rs` 或扩展现有 `ScoringAuditEvent`）：

```rust
struct ScoringEvent {
    schema_version: u32,          // 初始 1
    event_id: String,             // typed id 或 UUIDv7
    ts_unix_ms: u64,              // 或保留 RFC3339 timestamp 字段并双写过渡
    scenario: String,             // memory_rerank|memory_recall|skill_match|policy_triage
    outcome: ScoringEventOutcome, // Ok | Unavailable { kind }
    engine_revision: String,
    prompt_sha256: String,        // 与现指纹算法同源（state + questions 序列化）
    question_count: usize,
    latency_ms: u128,
    // 关联（缺省 null）
    correlation_id: Option<String>,
    session_id: Option<String>,
    run_ordinal: Option<u32>,
    step_ordinal: Option<u32>,
    tool_call_id: Option<String>,
    // 全量现场
    state_text: String,
    questions: Vec<ScoringQuestionSnapshot>, // 含选项全文
    answers: Vec<ScoringAnswerSnapshot>,     // probs / calibration / 选中或序
    ranking: Option<RankingSnapshot>,        // before/after 序（重排类场景）
    config_fingerprint: ScoringConfigFingerprint,
}
```

`ScoringQuestionSnapshot` **MUST** 按题型保留可复盘字段：

| 题型 | 必留 |
|---|---|
| Noul | 判据全文 / 阈值相关字段 |
| Choice | 各选项 id + **全文** + 最终 probabilities |
| Score | 评分维度说明 + probabilities 向量 |

`RankingSnapshot`（重排 / 匹配类）：

- `before_ids` / `after_ids`（或完整 before/after 选项序）
- 与 `answers` 中 probabilities **可互相核对**；**NEVER** 只留 sha 而无序

`ScoringConfigFingerprint` **MUST** 至少包含：场景开关（master + 该 scenario）、`event_retention_days`、与本次决策相关的阈值（如 policy_triage 温度/权重标识若已进配置）、引擎/权重 revision。缺省用显式 `None` / 默认哨兵，**NEVER** 静默省略关键开关。

### 5.2 与现状 `ScoringAuditEvent` 的演进

现状字段全部保留为超集中的诊断子集；新增现场与关联字段。推荐：

1. PR1：先落地日切 + `schema_version` + 全量现场（关联字段可全 `null`）。
2. PR2：装配透传 `ScoringCallContext`，填充关联字段。

旧单文件行无 `schema_version` → 读侧视为 `0` / legacy。

### 5.3 调用上下文透传

```rust
struct ScoringCallContext {
    correlation_id: Option<String>,
    session_id: Option<String>,
    run_ordinal: Option<u32>,
    step_ordinal: Option<u32>,
    tool_call_id: Option<String>,
}
```

- 装配点（composition / runtime 消费 `ScoringPort` 处）构造可选 context；**缺省不传**时事件写 `null`。
- **NEVER** 为补齐关联字段而失败评分或改变静默回退语义。
- Port 签名演进优先：装饰器持有 `Arc<dyn Fn() -> ScoringCallContext>` 或 per-call 扩展方法；具体形状实施时以最小破坏 `ScoringPort` 为准，但中心化 emit 出口仍只有审计装饰器一层。

## 6. 场景映射（emit 覆盖面）

四场景共用引擎、装配期注入 `scenario` 标签——事件逐条可归因。映射表（实施契约测试的强制清单）：

| scenario | 典型消费点 | 现场要点 |
|---|---|---|
| `memory_rerank` | Memory 重排 | state=查询/上下文；选项=候选记忆全文；before/after 序 |
| `memory_recall` | Memory 召回筛选 | state + 候选项；noul/score 结果 |
| `skill_match` | ToolSearch 语义重排 | state=用户意图；选项=skill 描述全文；before/after |
| `policy_triage` | Policy 预筛 | state=工具名+能力位等；noul 概率 + 阈值指纹 |

**MUST**：每个 scenario 至少一处生产装配路径在 `answer` 终态后产生事件（含 `Unavailable`）。开关关闭导致不调用 `ScoringPort` 的路径 **不** 要求伪造事件。

## 7. 分层落点

```text
domain/     ScoringEvent、ScoringEventOutcome、ScoringConfigFingerprint、
            ScoringQuestionSnapshot / AnswerSnapshot / RankingSnapshot
ports/      （可选）ScoringEventAppendPort { append(&ScoringEvent) -> Result<(), …> }
            测试-only 读回 helper 可挂同一 adapter，不进生产查询发布面
adapters/   AuditedScoringAdapter（中心化 emit）
            JsonlSegmentScoringEventStore（日切 + GC + legacy 迁移）
            calibration_store::append_jsonl_line_sync 可复用
wiring/     scoring_dir → events/；停写 AUDIT_FILE；注入 retention
composition/ 打开评分时注入 root + retention；构造 ScoringCallContext 源（PR2）
runtime/    消费点透传 session/run/step/correlation（PR2；无业务查询面）
```

`NoOp` / 开关关闭：不调用 Port 则无事件；装饰器未装配时 **MUST NOT** 要求事件流。

domain **NEVER** 直接 IO（守卫：I/O 只在 adapters）。

## 8. 配置

- `scoring.event_retention_days`：默认 `30`；仅影响 GC。
- 事件写入随评分审计装饰器装配而启用（与现 `AuditedScoringAdapter` 同生命周期）；评分总开关关闭导致无 `answer` 调用时无事件。
- 配置进 `ScoringConfigFingerprint`，便于复盘对照「当时开关」。

具体字段名以 `specs/3.9-config-compat.md` 实施时登记为准。

## 9. 交付拆分

### PR1 — schema + 写路径 + 日切 + 迁移 + GC

- 事件 schema 单测 + jsonl 落盘/读回契约（测试 helper）。
- 中心化 emit：`AuditedScoringAdapter` 改为写 `events/{date}.jsonl`；全量现场字段。
- 旧 `audit.jsonl` 迁移/旁路归档 + 停写。
- 30 天 retention/GC；fail-open 行为保持。
- `observations.jsonl` / 校准路径回归：行为不变。
- 关联字段可全 `null`（为 PR2 留位）。

### PR2 — 关联透传 + 四场景契约

- `ScoringCallContext`（或等价）从 composition/runtime 消费点透传。
- 四场景映射表契约测试闭环。
- 缺省 context → 字段 `null`；有 context → 字段齐全。
- **不含** 报告工具与生产查询 Port。

## 10. 验收

| 项 | 标准 |
|---|---|
| Schema | 单测覆盖序列化 / 缺省 / `schema_version` / 旧行兼容 |
| 落盘读回 | adapter 契约：append → 日文件行 → 反序列化相等（含 state/选项全文） |
| 迁移 | legacy `audit.jsonl` 停写；可解析行进入归档/segment；评分主路径绿 |
| 现场 | 新事件含 state 全文、全部选项正文、probabilities、ranking（适用场景） |
| 关联 | PR2 后：有 context 时四字段+correlation 非空；无 context 时显式 null 且评分成功 |
| 四场景 | 映射表测试：每个启用 scenario 至少一处生产路径 emit |
| 保留 | 过期 segment 被 GC；未过期保留 |
| 降级 | 强制 append 失败时 `answer` 仍返回原结果 |
| 校准旁路 | `observations.jsonl` 读写语义不变 |
| 报告 / 查询工具 | **明确不做**（follow-up） |

## 11. 与 Memory 事件流对照

| 维度 | Memory（`06-event-stream.md`） | SystemOne（本文） |
|---|---|---|
| 目标 | 决策现场复盘（write/read/reflection/lifecycle） | 评分现场复盘（每次 `answer`） |
| 既有事实流 | 无（新建）；另改 reflection-history | **升级替换** `audit.jsonl` |
| 路径 | `memory/{project_key}/events/{date}.jsonl` | `scoring/events/{date}.jsonl` |
| 内容 | 条目 before/after、候选全文 | state + 选项全文 + ranking + probs |
| 关联 | correlation / session / run / step / tool_call | **同套** |
| 保留 | 默认 30 天 | **同** |
| 报告面 | 本期不做 | **同** |
| emit | MemoryService 中心化 | AuditedScoringAdapter 中心化 |
| fail-open | 是 | **同**（已有） |
| 明文 | 是 | **同** |
| 旁路不动 | 运行日志无正文 | 运行日志无正文 + **observations 不动** |
| 交付 | 两 PR | **两 PR** |

## 12. 风险

| 风险 | 缓解 |
|---|---|
| 全量现场体积大（长候选） | 30 天硬 GC；后续采样另决策；监控单日文件体积 |
| Port 签名透传面大 | PR1/PR2 切开；缺省 null 不阻断 |
| legacy 迁移损坏 | 旁路归档 + 原文件改后缀；失败 fail-open |
| 与校准文件职责混淆 | 目录约定与文档边界写死；测试锁定 observations 路径 |
| domain-no-direct-io 守卫 | I/O 只在 adapters |
| 误把事件流当评测源 | 非目标写明；harness 保持独立 |

## 13. 相关文档

- [README](README.md) — 模块定位
- [01-systemone-scoring.md](01-systemone-scoring.md) — 三题型与 ScoringPort
- [02-kev-deployment.md](02-kev-deployment.md) — 部署与 scoring 目录（实施时同步审计路径）
- Memory 对照：[../memory/06-event-stream.md](../memory/06-event-stream.md)
- 评测 harness：`../../../../eval/system-one/`（独立，不依赖本事件流）
- 运行日志 target：`specs/3.15-logging.md`（`aemeath:agent:systemone`）
- Audit append 先例：`agent/features/audit/src/adapters/append.rs`

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-10-10 | 初稿：对照 Memory 事件流拍板；升级替换 audit；全量现场；两 PR；无报告面 |
