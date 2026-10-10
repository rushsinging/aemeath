# System One 可复盘评分事件流 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `~/.agents/scoring/audit.jsonl` 升级为按日 append-only 的可复盘评分事件流（全量现场 + 关联字段 + 30 天 GC）；校准旁路不动；本期不做查询/报告面。

**Architecture:** 保持 `AuditedScoringAdapter` 为唯一中心化 emit 出口（可抽薄 `ScoringEventAppend` / `JsonlSegmentScoringEventStore`）。PR1 落地 schema、日切写路径、legacy 迁移、GC、全量现场（关联可 null）。PR2 透传 `ScoringCallContext` 并补四场景映射契约测试。写失败 fail-open。评测仍走 `eval/system-one/` harness。

**Tech Stack:** Rust 2021、Tokio、serde_json jsonl、现有 `append_jsonl_line_sync`、systemone 六边形分层、`cargo test -p systemone`。

**Design doc:** `docs/design/02-modules/systemone/03-event-stream.md`

**Issue:** #1909

**对照:** Memory 事件流 `#1903` / `docs/design/02-modules/memory/06-event-stream.md`

---

## 文件地图

### PR1

- **Create:** `agent/features/systemone/src/domain/event.rs` — `ScoringEvent` / snapshots / fingerprint / outcome
- **Create:** `agent/features/systemone/src/domain/event_tests.rs`
- **Create:** `agent/features/systemone/src/adapters/event_jsonl.rs` — 日切 append + retention GC + legacy 迁移
- **Create:** `agent/features/systemone/src/adapters/event_jsonl_tests.rs`
- **Modify:** `agent/features/systemone/src/adapters/audited.rs` — emit 新事件模型到 `events/{date}.jsonl`；全量现场
- **Modify:** `agent/features/systemone/src/adapters/audited_tests.rs` — 路径/字段/fail-open/迁移
- **Modify:** `agent/features/systemone/src/constants.rs` — `EVENT_SCHEMA_VERSION`、`events/` 段名、默认 retention；`AUDIT_FILE` 标 legacy
- **Modify:** `agent/features/systemone/src/wiring.rs` / `lib.rs` — 注入 events 根与 retention
- **Modify:** `agent/shared/src/config/**` — `scoring.event_retention_days` 默认 30（若已有 Scoring 配置扩展点）
- **Modify:** `docs/design/02-modules/systemone/02-kev-deployment.md` — 若实施细节需再同步（设计稿已预留）
- **Modify:** `specs/3.9-config-compat.md` — 登记配置项（实施时）

### PR2

- **Create/Modify:** `ScoringCallContext` 类型 + 装饰器/Port 最小透传面
- **Modify:** composition / runtime 四场景消费点注入 context
- **Create:** `agent/features/systemone/src/event_coverage_tests.rs`（或 adapters 旁）— 四场景映射契约
- **Modify:** 相关 wiring 测试

---

## PR1 Tasks

### Task 1: 事件领域 schema

**Files:**
- Create: `agent/features/systemone/src/domain/event.rs`
- Create: `agent/features/systemone/src/domain/event_tests.rs`
- Modify: domain mod 导出、`constants.rs`（`EVENT_SCHEMA_VERSION = 1`）

- [ ] **Step 1: 写失败测试** — serde round-trip；缺省 Option→None；含 state/questions/answers/ranking 字段
- [ ] **Step 2: 运行测试确认失败** — `cargo test -p systemone event::`
- [ ] **Step 3: 实现领域类型**
- [ ] **Step 4: 运行测试确认通过**
- [ ] **Step 5: 提交**

### Task 2: 日切 jsonl store + GC + legacy 迁移

**Files:**
- Create: `adapters/event_jsonl.rs` + tests

- [ ] **Step 1: 写失败测试** — append 到 `events/{date}.jsonl`；GC 删过期；legacy `audit.jsonl` 迁入/归档后停写
- [ ] **Step 2: 红 → 实现 → 绿**
- [ ] **Step 3: 提交**

### Task 3: AuditedScoringAdapter 改写路径 + 全量现场

**Files:**
- Modify: `adapters/audited.rs` + `audited_tests.rs` + wiring

- [ ] **Step 1: 写失败测试** — 新事件含 state 全文与选项正文；fail-open；observations 路径不变
- [ ] **Step 2: 红 → 实现 → 绿**
- [ ] **Step 3: 提交**

### Task 4: 配置项 `scoring.event_retention_days`

- [ ] 默认 30；`0`=禁 GC；登记 specs；composition 注入
- [ ] 提交

---

## PR2 Tasks

### Task 5: ScoringCallContext 透传

- [ ] 类型 + 装饰器持有/per-call 注入（选最小破坏面）
- [ ] 无 context → 字段 null 且评分成功
- [ ] 有 context → correlation/session/run/step 写入事件
- [ ] 提交

### Task 6: 四场景映射契约测试

- [ ] memory_rerank / memory_recall / skill_match / policy_triage 各至少一处生产路径 emit（启用时）
- [ ] 提交

---

## 验收对照（设计文档 §10）

实施完成后按 `03-event-stream.md` §10 表格逐项勾选；**NEVER** 声称完成而未跑 `cargo test -p systemone` 相关套件。

## 非目标（提醒）

- 查询 Port / TUI / slash / 在线报告
- 改 observations / calibration 语义
- 把事件流接入 eval harness 作为验收源
