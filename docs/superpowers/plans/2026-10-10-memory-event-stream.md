# Memory 事件流 + reflection-history append-only Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Memory 建立含内容的真 append-only 事件流（19 项操作面落点），并把 `reflection-history` 从整 Vec CAS 改为同形态 jsonl append-only；本期不做查询/报告面，供 LLM 事后读文件复盘。

**Architecture:** Memory-owned `MemoryEventAppendPort` + `JsonlSegmentEventStore`（复用 `SafeStorageRoot` append，先例 Audit `FileUsageAppendStore`）。`MemoryService` / opener / `ReflectionWorkflow` 中心化 emit，写失败 fail-open。`reflection-history` 换 `JsonlReflectionHistoryStore`：append/upsert 只追加，`list` 按 id 折叠最新；旧 AtomicDataset Vec 一次性迁移。保留默认 30 天 segment GC。分两 PR 交付。

**Tech Stack:** Rust 2021、Tokio、serde_json jsonl、`storage::SafeStorageRoot`、现有 memory 六边形分层（domain ← application ← ports ← adapters）、`cargo test -p memory`。

**Design doc:** `docs/design/02-modules/memory/06-event-stream.md`

**Issue:** #1903

---

## 文件地图

### PR1

- **Create:** `agent/features/memory/src/domain/event.rs` — `MemoryEvent` / `MemoryEventOp` / `EventOutcome` / `ConfigFingerprint` / `EventChange` / `EventContext`
- **Create:** `agent/features/memory/src/domain/event_tests.rs` — schema 序列化与不变量
- **Create:** `agent/features/memory/src/ports/event_append.rs` — `MemoryEventAppendPort` + 错误类型
- **Create:** `agent/features/memory/src/adapters/event_jsonl.rs` — 按日 jsonl append + cfg(test) 读回 + retention GC
- **Create:** `agent/features/memory/src/adapters/event_jsonl_tests.rs`
- **Create:** `agent/features/memory/src/adapters/reflection_history_jsonl.rs` — 新 history 实现
- **Create:** `agent/features/memory/src/adapters/reflection_history_jsonl_tests.rs`
- **Modify:** `agent/features/memory/src/domain.rs`（或 mod 树）导出 event
- **Modify:** `agent/features/memory/src/ports.rs` — 挂 EventAppendPort；调整 history 文档语义
- **Modify:** `agent/features/memory/src/adapters.rs` — mod + wire
- **Modify:** `agent/features/memory/src/constants.rs` — segment 名、默认 retention、schema_version
- **Modify:** `agent/features/memory/src/service.rs` — 注入 append port；写组 + OpenLoad/CommitCas emit
- **Modify:** `agent/features/memory/src/service_tests.rs`（及必要拆分）— fail-open、写组落点
- **Modify:** `agent/features/memory/src/lib.rs` — `wire_memory_event_store` / 调整 `wire_reflection_history_store`
- **Modify:** `agent/features/memory/tests/reflection_history_adapter.rs` — 对齐 append-only
- **Modify:** composition 打开 Memory 处 — 注入 `SafeStorageRoot` + retention（路径以代码搜索 `wire_memory_opener` / `wire_reflection_history_store` 为准）
- **Modify:** `agent/shared/src/config/**` — `event_retention_days` 默认 30（若配置层已有 MemoryConfig 扩展点）
- **Modify:** `docs/design/02-modules/memory/03-reflection.md`、`04-ports-and-adapters.md` — 同步 append-only / EventAppendPort（无外部追踪号）

### PR2

- **Modify:** `service.rs` / `application/recall.rs` / `application.rs` — 读组 + 反思组 + 剩余生命周期 emit
- **Create:** `agent/features/memory/src/event_coverage_tests.rs`（或 adapters 旁）— 19 项映射表契约
- **Modify:** 相关 service/application 测试

---

## PR1 Tasks

### Task 1: 事件领域 schema

**Files:**
- Create: `agent/features/memory/src/domain/event.rs`
- Create: `agent/features/memory/src/domain/event_tests.rs`
- Modify: `agent/features/memory/src/domain.rs`（或现有 domain mod 入口）
- Modify: `agent/features/memory/src/constants.rs` — `EVENT_SCHEMA_VERSION = 1`

- [ ] **Step 1: 写失败测试**

覆盖：`MemoryEvent` serde round-trip；`MemoryEventOp` 枚举含读/写/反思/生命周期全部判别式（可先列齐 19 个命名）；缺省 `Option` 字段反序列化为 `None`；`schema_version` 写入。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p memory event::`
Expected: 编译失败（模块不存在）。

- [ ] **Step 3: 实现领域类型**

按设计文档 §5 落地最小可序列化结构；`EventChange` 用枚举区分 Write { before, after } / Read { candidates: Vec<MemoryEntry>, … } / Reflection { … } / Lifecycle { … }，避免一个巨型 Optional 包。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p memory event::`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add agent/features/memory/src/domain/event.rs agent/features/memory/src/domain/event_tests.rs agent/features/memory/src/domain.rs agent/features/memory/src/constants.rs
git commit -m "$(cat <<'EOF'
feat(memory): add MemoryEvent schema for append-only telemetry

EOF
)"
```

---

### Task 2: EventAppendPort + 空实现

**Files:**
- Create: `agent/features/memory/src/ports/event_append.rs`
- Modify: `agent/features/memory/src/ports.rs`
- Create: 测试内 `NoopEventAppend` 或 ports 旁 test helper

- [ ] **Step 1: 写失败测试**

定义 `RecordingEventAppend`（Arc\<Mutex\<Vec\<MemoryEvent>>>）实现 Port，断言 `append` 推入事件；`Noop` 成功且无副作用。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p memory event_append`
Expected: 失败/编译失败。

- [ ] **Step 3: 实现 Port trait**

```rust
#[async_trait]
pub trait MemoryEventAppendPort: Send + Sync {
    async fn append(&self, event: &MemoryEvent) -> Result<(), EventAppendError>;
}
```

错误类型不携带记忆正文。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p memory event_append`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add agent/features/memory/src/ports/event_append.rs agent/features/memory/src/ports.rs
git commit -m "$(cat <<'EOF'
feat(memory): add MemoryEventAppendPort

EOF
)"
```

---

### Task 3: JsonlSegmentEventStore（落盘 + 读回 + GC）

**Files:**
- Create: `agent/features/memory/src/adapters/event_jsonl.rs`
- Create: `agent/features/memory/src/adapters/event_jsonl_tests.rs`
- Modify: `agent/features/memory/src/adapters.rs`
- Modify: `agent/features/memory/src/constants.rs` — `EVENTS_SEGMENT = "events"`、`EVENT_JSONL_SUFFIX`、`DEFAULT_EVENT_RETENTION_DAYS = 30`
- Modify: `agent/features/memory/src/lib.rs` — `wire_memory_event_store(root, project, retention_days)`

- [ ] **Step 1: 写失败测试**

使用 tempfile + `SafeStorageRoot`（与 audit/memory 现有测试同一打开方式）：
1. append 一条 → 日文件存在 → 读回相等；
2. 两行 append 顺序保留；
3. 非法跨行 payload 拒绝（若做校验）；
4. GC：写入「过期文件名」与「今日文件」，`retain` 后仅今日保留；
5. 路径含 `memory/{project}/events/`。

对照实现：`agent/features/audit/src/adapters/append.rs`（`create_or_open` + `append: true` + 行校验）。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p memory event_jsonl`
Expected: 编译失败。

- [ ] **Step 3: 实现 adapter**

- 日文件名 UTC `yyyy-mm-dd.jsonl`
- 进程内按文件 key 加锁
- `gc_expired(now, retention_days)` 删除过期 segment
- cfg(test) `read_all_for_test` 供契约用，**不**进 `memory::api`

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p memory event_jsonl`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add agent/features/memory/src/adapters/event_jsonl.rs agent/features/memory/src/adapters/event_jsonl_tests.rs agent/features/memory/src/adapters.rs agent/features/memory/src/constants.rs agent/features/memory/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(memory): persist events as daily append-only jsonl segments

EOF
)"
```

---

### Task 4: MemoryService 注入 emit 骨架 + fail-open

**Files:**
- Modify: `agent/features/memory/src/service.rs`
- Modify: `agent/features/memory/src/adapters.rs`（opener 构造 Service 时注入）
- Modify: `agent/features/memory/src/service_tests.rs`

- [ ] **Step 1: 写失败测试**

1. 注入 `RecordingEventAppend`，调用任意已接线的 write，断言至少一条事件；
2. 注入恒 `Err` 的 append port，`write` / `retrieve_for_inject` 仍成功。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p memory service::`
Expected: 失败（无 emit 字段）。

- [ ] **Step 3: 实现中心化 helper**

```rust
async fn emit_event(&self, event: MemoryEvent) {
    if let Err(error) = self.events.append(&event).await {
        log::warn!(target: LOG_TARGET, "memory_event_append_failed op={:?} err={error}");
    }
}
```

Service 增加 `Arc<dyn MemoryEventAppendPort>`；测试默认 Recording 或 Noop。Opener 未装配时用 Noop，避免测试大爆炸——再在 composition 接真 store。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p memory`
Expected: PASS（既有 + 新 fail-open）。

- [ ] **Step 5: 提交**

```bash
git add agent/features/memory/src/service.rs agent/features/memory/src/adapters.rs agent/features/memory/src/service_tests.rs
git commit -m "$(cat <<'EOF'
feat(memory): centralize event emit with fail-open

EOF
)"
```

---

### Task 5: 写组 8 项落点

**Files:**
- Modify: `agent/features/memory/src/service.rs`
- Modify: `agent/features/memory/src/service_tests.rs`

- [ ] **Step 1: 写失败测试**

对 `write` / `update` / `delete` / `pin` / `mark_outdated` / `archive`+`restore` / `compact` / supersede-or-synthesis 路径各断言对应 `MemoryEventOp` 与 before/after（或 affected）含正文。用 Recording port。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p memory event_write_ops`
Expected: 断言失败。

- [ ] **Step 3: 在各 commit 成功路径 emit**

失败路径也 emit `outcome=Failed`（无阻断）。共用 `correlation_id`（CAS 重试同链）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p memory event_write_ops`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add agent/features/memory/src/service.rs agent/features/memory/src/service_tests.rs
git commit -m "$(cat <<'EOF'
feat(memory): emit events for all write-path operations

EOF
)"
```

---

### Task 6: 生命周期 OpenLoad + CommitCas

**Files:**
- Modify: opener / `service.rs` commit 路径
- Modify: 对应测试

- [ ] **Step 1: 写失败测试**

打开 Memory 产生 `OpenLoad`；一次成功 commit 产生 `CommitCas`（含 revision）。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p memory event_lifecycle`
Expected: 失败。

- [ ] **Step 3: 实现 emit**

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p memory event_lifecycle`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add agent/features/memory/src/service.rs agent/features/memory/src/adapters.rs agent/features/memory/src/service_tests.rs
git commit -m "$(cat <<'EOF'
feat(memory): emit OpenLoad and CommitCas lifecycle events

EOF
)"
```

---

### Task 7: reflection-history 真 append-only

**Files:**
- Create: `agent/features/memory/src/adapters/reflection_history_jsonl.rs`
- Create: `agent/features/memory/src/adapters/reflection_history_jsonl_tests.rs`
- Modify: `agent/features/memory/src/lib.rs` — `wire_reflection_history_store` 改为 jsonl 实现（参数改为 `SafeStorageRoot` + project，或同时持有 dataset port 仅用于 legacy 迁移）
- Modify: `agent/features/memory/tests/reflection_history_adapter.rs`
- Modify: composition 调用点
- Modify: `docs/design/02-modules/memory/03-reflection.md` — history 段落改为 append-only + 读折叠

- [ ] **Step 1: 写失败测试**

1. append 两条不同 id → list(10) 两条；
2. upsert 同 id 两次 → 文件两行、list 折叠为最新内容；
3. legacy：预置旧 AtomicDataset Vec member → 首次 list/append 迁移到 jsonl 且可读；
4. 保留窗 GC 删过期 history segment。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p memory reflection_history_jsonl`
Expected: 失败。

- [ ] **Step 3: 实现 JsonlReflectionHistoryStore**

保留 `ReflectionHistoryStore` trait 签名；内部不再 `mutate_records` 整 Vec。迁移逻辑只读旧 dataset一次。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p memory reflection_history`
Expected: PASS（单元 + tests/ 集成）。

- [ ] **Step 5: 提交**

```bash
git add agent/features/memory/src/adapters/reflection_history_jsonl.rs agent/features/memory/src/adapters/reflection_history_jsonl_tests.rs agent/features/memory/src/adapters.rs agent/features/memory/src/lib.rs agent/features/memory/tests/reflection_history_adapter.rs docs/design/02-modules/memory/03-reflection.md
# 加上 composition 实际改动文件
git commit -m "$(cat <<'EOF'
feat(memory): store reflection history as append-only jsonl

EOF
)"
```

---

### Task 8: 配置 retention + composition 接线 + 设计文档端口同步

**Files:**
- Modify: `agent/shared/src/config/**`（MemoryConfig 字段）
- Modify: composition 装配
- Modify: `docs/design/02-modules/memory/04-ports-and-adapters.md`
- Modify: `specs/3.9-config-compat.md`（若新增配置项）

- [ ] **Step 1: 写失败测试**

配置默认 `event_retention_days == 30`；wire 后打开的 Memory 使用真 Event store（集成测或 composition 测选最窄层）。

- [ ] **Step 2: 运行测试确认失败**

Run: 针对改动 crate 的最窄 `cargo test -p …`
Expected: 失败。

- [ ] **Step 3: 接线**

`wire_memory_event_store` + opener/service 注入；启动时可选触发一次 GC。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p memory` 与改动到的 config/composition 测试
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git commit -m "$(cat <<'EOF'
feat(memory): wire event store and retention config

EOF
)"
```

---

### Task 9: PR1 验证门禁

- [ ] **Step 1: 跑 memory 全量**

Run: `cargo test -p memory`
Expected: PASS。

- [ ] **Step 2: clippy 窄范围**

Run: `cargo clippy -p memory --all-targets -- -D warnings`
Expected: 无警告。

- [ ] **Step 3: 打开 PR1**

标题建议：`feat(memory): append-only event stream (write path) + reflection-history jsonl`
正文链接设计文档与 #1903；注明报告面不做、PR2 补齐读/反思。

---

## PR2 Tasks

### Task 10: 读组 4 项（含候选正文）

**Files:**
- Modify: `agent/features/memory/src/service.rs` — retrieve/search/list/stats
- Modify: `agent/features/memory/src/application/recall.rs` — PerMessageRecall
- Modify: 对应 tests

- [ ] **Step 1: 写失败测试** — 四条路径 Recording 断言 op + candidates 正文非空（有命中时）
- [ ] **Step 2: 运行失败** — `cargo test -p memory event_read_ops`
- [ ] **Step 3: 实现 emit**
- [ ] **Step 4: 运行通过**
- [ ] **Step 5: 提交** `feat(memory): emit full-content events on read paths`

---

### Task 11: 反思组 3 项

**Files:**
- Modify: `agent/features/memory/src/application.rs`（ReflectionWorkflow）
- Modify: `service.rs` apply 边界（若 cost/apply 分落）
- Modify: `service_reflection_tests.rs` / application tests

- [ ] **Step 1: 写失败测试** — Triggered / Applied / Cost 三事件字段（区间、建议数、token/duration）
- [ ] **Step 2: 运行失败**
- [ ] **Step 3: 实现 emit**
- [ ] **Step 4: 运行通过**
- [ ] **Step 5: 提交** `feat(memory): emit reflection trigger/apply/cost events`

---

### Task 12: 剩余生命周期 + 19 项映射表契约

**Files:**
- Modify: service/opener — `AssemblyFingerprint` / `EvictionWatermark`
- Create: `agent/features/memory/src/event_coverage_tests.rs` — 常量表列出 19 op，断言测试套件或代码路径覆盖集相等

- [ ] **Step 1: 写失败测试** — 映射表与「已有 emit 测试覆盖的 op 集合」一致
- [ ] **Step 2: 运行失败**
- [ ] **Step 3: 补齐缺失 emit + 表**
- [ ] **Step 4: 运行** `cargo test -p memory event_coverage`
- [ ] **Step 5: 提交** `test(memory): lock 19-op event coverage map`

---

### Task 13: PR2 验证门禁

- [ ] **Step 1:** `cargo test -p memory`
- [ ] **Step 2:** `cargo clippy -p memory --all-targets -- -D warnings`
- [ ] **Step 3:** 打开 PR2，关闭 #1903 验收（报告 AC 标为 follow-up / out of scope）

---

## 执行手顺注意

- **TDD：** 每任务先红后绿；跨层改动每层有测（AGENTS constitution）。
- **守卫：** I/O 只在 `adapters/`；勿在 domain/service 直接 `std::fs`。
- **格式：** 不手调 rustfmt；逻辑提交后让 rustfmt/hook 处理。
- **Worktree：** 已在 `design-1903-memory-telemetry`；实施可同分支续做或按工作流再切 feature 分支。
- **提交信息：** 遵循仓库近期 `feat(memory):` / `test(memory):` 风格；NEVER 在设计文档正文新加 Issue 号（计划与 PR 正文可引用 #1903）。

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-10-10-memory-event-stream.md`.
