# Audit Usage JSONL 单一事实实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除 Audit worker metrics 与共享 shutdown 终态，以消费式 worker owner 保证一次性 drain/abort，并保持按每条 record 的 Session 分区写入及全局 UsageQuery。

**Architecture:** `UsageSender` 仅共享可关闭的 bounded channel slot；无状态 worker 顺序按 `UsageRecord.session_id` 派生 JSONL stream。`UsageWorker::shutdown(self)` 消费唯一 owner，在正常路径 drain、超时或 Future 取消时 abort；JSONL 是唯一持久事实，全局查询继续从全部分区即时计算。

**Tech Stack:** Rust 2021、Tokio mpsc/JoinHandle/time、async-trait、serde_json、Audit AppendLog port、Cargo、Bash architecture guards。

---

## 文件结构

### 修改

- `agent/features/audit/src/application/ingest.rs`：无指标 sender、无状态 worker、消费式 shutdown、Drop abort。
- `agent/features/audit/src/application/ingest_tests.rs`：worker 局部行为与取消/timeout 测试。
- `agent/features/audit/src/application.rs`：收窄 application façade。
- `agent/features/audit/src/lib.rs`：收窄 crate-root façade。
- `agent/features/audit/tests/usage_worker_contract.rs`：公开消费式 worker 契约。
- `agent/features/audit/tests/usage_query_contract.rs`：适配消费式 shutdown，并冻结跨 Session 全局查询/汇总。
- `agent/composition/src/audit.rs`：`SessionAudit` 改为消费 owner；重命名 assembly 字段。
- `agent/composition/src/runtime.rs`：保持一套 frontend/runtime worker，并从 canonical records 分区。
- `agent/composition/src/app.rs`：bootstrap 继续转交 Audit owner。
- `agent/composition/tests/audit_worker_assembly.rs`：跨 Session JSONL 与消费式 shutdown 相邻边界。
- `apps/cli/src/chat.rs`：从 bootstrap 移出并消费 `SessionAudit`。
- `apps/cli/src/chat_tests.rs`：保留 frontend result 与 drain 正交契约。
- `.agents/hooks/check-crate-api-boundary.sh`：删除退役公开符号、登记 `UsageWorker`。
- `docs/design/03-engineering/01-architecture-guards.md`：同步 Audit façade 可读索引。
- `docs/design/02-modules/audit/README.md`：用 JSONL 单一事实与消费式 drain 替代指标状态机。
- `docs/design/02-modules/audit/01-usage-storage.md`：回写测试矩阵、Current 状态和验收结论。
- `docs/superpowers/specs/2026-08-24-audit-session-jsonl-only-design.md`：仅在实施证据要求澄清时同步，不改变已批准边界。

不新增生产模块，不改变 JSONL schema 或存储路径。

---

### Task 1: 用失败测试冻结无指标、消费式 worker 行为

**Files:**
- Modify: `agent/features/audit/src/application/ingest_tests.rs`
- Test: `agent/features/audit/src/application/ingest_tests.rs`

- [ ] **Step 1: 将测试 fixture 固定为两个明确 Session**

把现有 `record(id)` 改为显式 Session 参数，避免把 record ID 误当作 worker 身份：

```rust
fn record(session_id: &str, id: &str) -> UsageRecord {
    UsageRecord {
        recorded_at_unix_ms: 1,
        session_id: SessionId::new(session_id),
        run_id: RunId::new(format!("run-{id}")),
        run_step_id: RunStepId::new(format!("step-{id}")),
        model_invocation_id: ModelInvocationId::new(format!("invocation-{id}")),
        provider: "test-provider".to_string(),
        model: "test-model".to_string(),
        input_tokens: 1,
        output_tokens: 2,
        cache_write_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens: None,
    }
}
```

所有既有调用改为 `record("session-a", "first")` 等明确输入。

- [ ] **Step 2: 改写 FIFO 测试，证明同一 worker 按 record Session 分区且不依赖 metrics**

用以下测试替换 `worker_calls_append_then_flush_in_fifo_order_and_drains`：

```rust
#[tokio::test]
async fn worker_partitions_each_record_by_session_and_drains_in_fifo_order() {
    let (store, mut append_started, _append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );
    let first = record("session-a", "first");
    let second = record("session-b", "second");

    assert_eq!(sender.try_record(first.clone()), UsageEmitOutcome::Accepted);
    assert_eq!(sender.try_record(second.clone()), UsageEmitOutcome::Accepted);
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    worker.shutdown().await;
    let first_stream = AppendLogStream::for_session(&first.session_id)
        .as_str()
        .to_string();
    let second_stream = AppendLogStream::for_session(&second.session_id)
        .as_str()
        .to_string();
    assert_eq!(
        store.calls(),
        vec![
            StoreCall::Append {
                stream: first_stream.clone(),
                terminated: true,
            },
            StoreCall::Flush {
                stream: first_stream,
            },
            StoreCall::Append {
                stream: second_stream.clone(),
                terminated: true,
            },
            StoreCall::Flush {
                stream: second_stream,
            },
        ]
    );
}
```

- [ ] **Step 3: 改写失败隔离测试，删除所有累计指标断言**

append 失败测试仅断言两条 record 都被 append、没有 flush：

```rust
#[tokio::test]
async fn append_failure_skips_flush_and_continues_with_next_record() {
    let (store, mut append_started, _append_dropped) = ControlledStore::new(true, false);
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("session-a", "second")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    worker.shutdown().await;
    let calls = store.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .all(|call| matches!(call, StoreCall::Append { .. })));
}
```

flush 失败测试仅断言两条 record 都执行 append 与 flush：

```rust
#[tokio::test]
async fn flush_failure_continues_with_next_record() {
    let (store, mut append_started, _append_dropped) = ControlledStore::new(false, true);
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("session-a", "second")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    worker.shutdown().await;
    let calls = store.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, StoreCall::Append { .. }))
            .count(),
        2
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, StoreCall::Flush { .. }))
            .count(),
        2
    );
}
```

- [ ] **Step 4: 添加可观察 Future drop 的 store fixture**

在测试文件增加：

```rust
struct AppendDropNotice(Option<mpsc::UnboundedSender<()>>);

impl Drop for AppendDropNotice {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
```

为 `ControlledStore` 增加 `append_dropped: mpsc::UnboundedSender<()>`，并让 `new` 返回第三项 receiver：

```rust
fn new(
    fail_append: bool,
    fail_flush: bool,
) -> (
    Arc<Self>,
    mpsc::UnboundedReceiver<()>,
    mpsc::UnboundedReceiver<()>,
) {
    let (append_started, started_receiver) = mpsc::unbounded_channel();
    let (append_dropped, dropped_receiver) = mpsc::unbounded_channel();
    (
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            append_started,
            append_dropped,
            allow_append: Semaphore::new(0),
            fail_append,
            fail_flush,
        }),
        started_receiver,
        dropped_receiver,
    )
}
```

在 `append` 的 semaphore await 前创建 guard：

```rust
let _drop_notice = AppendDropNotice(Some(self.append_dropped.clone()));
```

现有测试忽略第三个返回值：`let (store, mut append_started, _append_dropped) = ...`。

- [ ] **Step 5: 用 timeout 测试替换精确 outcome/metrics 测试**

```rust
#[tokio::test(start_paused = true)]
async fn shutdown_timeout_aborts_blocked_worker_and_closes_sender() {
    let (store, mut append_started, mut append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) = start_usage_worker(
        store,
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("append starts");

    worker.shutdown().await;

    append_dropped.recv().await.expect("blocked append future drops");
    assert_eq!(
        sender.try_record(record("session-a", "late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}
```

- [ ] **Step 6: 添加取消与普通 Drop 的回归测试**

```rust
#[tokio::test]
async fn cancelling_shutdown_aborts_worker_instead_of_detaching_it() {
    let (store, mut append_started, mut append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) = start_usage_worker(
        store,
        UsageWorkerConfig::new(1, Duration::from_secs(30)),
    );
    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("append starts");

    let shutdown_task = tokio::spawn(worker.shutdown());
    tokio::task::yield_now().await;
    shutdown_task.abort();
    let _ = shutdown_task.await;

    append_dropped.recv().await.expect("append future drops on cancellation");
    assert_eq!(
        sender.try_record(record("session-a", "late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}

#[tokio::test]
async fn dropping_worker_owner_aborts_worker_and_closes_sender() {
    let (store, mut append_started, mut append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) = start_usage_worker(
        store,
        UsageWorkerConfig::new(1, Duration::from_secs(30)),
    );
    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("append starts");

    drop(worker);

    append_dropped.recv().await.expect("append future drops with owner");
    assert_eq!(
        sender.try_record(record("session-a", "late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}
```

- [ ] **Step 7: 运行 RED 测试**

Run:

```bash
cargo test -p audit application::ingest_tests -- --nocapture
```

Expected: FAIL to compile because `shutdown()` still borrows `&self` and returns `UsageShutdownOutcome`; metrics-era implementation also cannot guarantee owner Drop/cancellation abort。

- [ ] **Step 8: 提交 RED 测试**

```bash
git add agent/features/audit/src/application/ingest_tests.rs
git commit -m "test(audit): #1596 冻结 JSONL-only worker 生命周期"
```

---

### Task 2: 实现无指标 sender 与消费式 UsageWorker

**Files:**
- Modify: `agent/features/audit/src/application/ingest.rs`
- Modify: `agent/features/audit/src/application.rs`
- Modify: `agent/features/audit/src/lib.rs`
- Test: `agent/features/audit/src/application/ingest_tests.rs`

- [ ] **Step 1: 用 channel slot 替换 PipelineState**

在 `ingest.rs` 保留配置类型，删除 `Lifecycle`、`PipelineState`、`UsagePipelineMetricsSnapshot`、`UsageShutdownOutcome` 和 `warn_at_threshold`。新增：

```rust
type UsageSenderSlot = Arc<Mutex<Option<mpsc::Sender<UsageRecord>>>>;

#[derive(Clone)]
pub struct UsageSender {
    sender: UsageSenderSlot,
}

impl UsageSender {
    pub fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome {
        let Ok(mut sender_slot) = self.sender.lock() else {
            return UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable);
        };
        let Some(sender) = sender_slot.as_ref() else {
            return UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable);
        };
        match sender.try_send(record) {
            Ok(()) => UsageEmitOutcome::Accepted,
            Err(mpsc::error::TrySendError::Full(_)) => {
                UsageEmitOutcome::Dropped(UsageDropReason::QueueFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                *sender_slot = None;
                UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
            }
        }
    }
}
```

- [ ] **Step 2: 定义唯一 owner `UsageWorker`**

```rust
pub struct UsageWorker {
    sender: UsageSenderSlot,
    timeout: Duration,
    join: Option<JoinHandle<()>>,
}

impl UsageWorker {
    pub async fn shutdown(mut self) {
        self.close_sender();
        let timeout = self.timeout;
        let Some(join) = self.join.as_mut() else {
            return;
        };
        if tokio::time::timeout(timeout, join).await.is_err() {
            self.join
                .as_ref()
                .expect("usage worker join remains owned during timeout")
                .abort();
            let _ = self
                .join
                .as_mut()
                .expect("aborted usage worker join remains owned")
                .await;
            log::warn!(
                target: crate::LOG_TARGET,
                "usage_pipeline kind=shutdown_timeout"
            );
        }
        self.join = None;
    }

    fn close_sender(&self) {
        if let Ok(mut sender_slot) = self.sender.lock() {
            sender_slot.take();
        }
    }
}

impl Drop for UsageWorker {
    fn drop(&mut self) {
        self.close_sender();
        if let Some(join) = self.join.take() {
            join.abort();
            log::warn!(
                target: crate::LOG_TARGET,
                "usage_pipeline kind=owner_dropped_before_shutdown"
            );
        }
    }
}
```

关键约束：timeout await 必须借用 `self.join.as_mut()`，不能提前 `take()` 到局部变量，否则 shutdown Future 被取消时 Drop 无法 abort task。

- [ ] **Step 3: 收窄 worker factory**

```rust
pub fn start_usage_worker(
    store: Arc<dyn UsageAppendStorePort>,
    config: UsageWorkerConfig,
) -> (UsageSender, UsageWorker) {
    let (sender, receiver) = mpsc::channel(config.capacity());
    let sender = Arc::new(Mutex::new(Some(sender)));
    let join = tokio::spawn(run_worker(receiver, store));
    (
        UsageSender {
            sender: Arc::clone(&sender),
        },
        UsageWorker {
            sender,
            timeout: config.shutdown_timeout(),
            join: Some(join),
        },
    )
}
```

- [ ] **Step 4: 删除 run_worker 的 metrics 参数与累计逻辑**

```rust
async fn run_worker(
    mut receiver: mpsc::Receiver<UsageRecord>,
    store: Arc<dyn UsageAppendStorePort>,
) {
    while let Some(record) = receiver.recv().await {
        let stream = AppendLogStream::for_session(&record.session_id);
        let bytes = match encode(&record) {
            Ok(bytes) => bytes,
            Err(_) => {
                log::warn!(target: crate::LOG_TARGET, "usage_pipeline kind=encode");
                continue;
            }
        };
        if store.append(&stream, &bytes).await.is_err() {
            log::warn!(target: crate::LOG_TARGET, "usage_pipeline kind=append");
            continue;
        }
        if store.flush(&stream).await.is_err() {
            log::warn!(target: crate::LOG_TARGET, "usage_pipeline kind=flush");
        }
    }
}
```

日志等级依据 `specs/3.15-logging.md` §3.15.4.3：Audit 持久化失败属于可恢复但存在数据缺口风险，使用 `warn`；不输出 record 原文。

- [ ] **Step 5: 收窄 re-export**

`application.rs` 改为：

```rust
pub use ingest::{start_usage_worker, UsageSender, UsageWorker, UsageWorkerConfig};
```

`lib.rs` 对应改为：

```rust
pub use application::{start_usage_worker, UsageSender, UsageWorker, UsageWorkerConfig};
```

确认 `UsagePipelineMetricsSnapshot`、`UsageShutdownOutcome`、`UsageWorkerHandle` 不再导出。

- [ ] **Step 6: 运行 GREEN 测试**

Run:

```bash
cargo test -p audit application::ingest_tests -- --nocapture
```

Expected: all ingest tests PASS，且 timeout/取消/Drop 测试都收到 append future drop 通知。

- [ ] **Step 7: 运行 Audit lib clippy**

Run:

```bash
cargo clippy -p audit --lib -- -D warnings
```

Expected: PASS，无死代码或 warning。

- [ ] **Step 8: 提交实现**

```bash
git add agent/features/audit/src/application/ingest.rs \
  agent/features/audit/src/application.rs \
  agent/features/audit/src/lib.rs
git commit -m "refactor(audit)!: #1596 仅保留 JSONL worker 事实"
```

---

### Task 3: 收窄公开 worker 契约并冻结全局 UsageQuery

**Files:**
- Modify: `agent/features/audit/tests/usage_worker_contract.rs`
- Modify: `agent/features/audit/tests/usage_query_contract.rs`
- Test: same files

- [ ] **Step 1: 改写公开 worker 契约**

删除 `UsageShutdownOutcome` import 和 metrics 断言。让两个 records 使用两个 Session，并验证 stream 顺序：

```rust
#[tokio::test]
async fn public_worker_partitions_records_drains_once_and_rejects_late_records() {
    let store = Arc::new(RecordingStore::default());
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(4, Duration::from_secs(1)),
    );
    let first = record("a");
    let second = record("b");
    let first_stream = AppendLogStream::for_session(&first.session_id)
        .as_str()
        .to_string();
    let second_stream = AppendLogStream::for_session(&second.session_id)
        .as_str()
        .to_string();

    assert_eq!(sender.try_record(first), UsageEmitOutcome::Accepted);
    assert_eq!(sender.try_record(second), UsageEmitOutcome::Accepted);
    worker.shutdown().await;

    assert_eq!(
        store.calls.lock().expect("store calls lock").as_slice(),
        [
            format!("append:{first_stream}:true"),
            format!("flush:{first_stream}"),
            format!("append:{second_stream}:true"),
            format!("flush:{second_stream}"),
        ]
    );
    assert_eq!(
        sender.try_record(record("late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}
```

- [ ] **Step 2: 改写现有 sender→file→query 场景的 shutdown**

在 `usage_query_contract.rs`：

- 删除 `UsageShutdownOutcome` import；
- 将 `handle` 重命名为 `worker`；
- 将 outcome 断言改为 `worker.shutdown().await;`。

- [ ] **Step 3: 新增跨 Session 全局 query/summary 场景**

```rust
#[tokio::test]
async fn global_query_reads_and_summarizes_records_from_multiple_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(file_usage_append_store(
        SafeStorageRoot::open(temp.path()).unwrap(),
    ));
    let service = usage_query_service(store.clone());
    let first = record("session-global-a", "match", 15);
    let second = record("session-global-b", "match", 16);
    let (sender, worker) =
        start_usage_worker(store, UsageWorkerConfig::new(4, Duration::from_secs(1)));

    assert_eq!(sender.try_record(first.clone()), audit::UsageEmitOutcome::Accepted);
    assert_eq!(sender.try_record(second.clone()), audit::UsageEmitOutcome::Accepted);
    worker.shutdown().await;

    let page = service.query(query(10)).await.unwrap();
    assert_eq!(page.records.len(), 2);
    assert!(page.records.contains(&first));
    assert!(page.records.contains(&second));
    let summary = service.summarize(query(10)).await.unwrap();
    assert_eq!(summary.record_count, 2);
    assert_eq!(summary.input_tokens, 20);
    assert_eq!(summary.output_tokens, 40);
    assert_eq!(summary.cache_write_tokens, 6);
    assert_eq!(summary.cache_read_tokens, 0);
    assert_eq!(summary.reasoning_tokens, 10);
}
```

- [ ] **Step 4: 运行契约测试**

Run:

```bash
cargo test -p audit --test usage_worker_contract -- --nocapture
cargo test -p audit --test usage_query_contract -- --nocapture
```

Expected: both PASS；全局 query 从两个 JSONL 分区返回并汇总两条 record。

- [ ] **Step 5: 提交契约变更**

```bash
git add agent/features/audit/tests/usage_worker_contract.rs \
  agent/features/audit/tests/usage_query_contract.rs
git commit -m "test(audit): #1596 冻结跨 Session JSONL 查询契约"
```

---

### Task 4: 在 Composition 与 Runtime 全链路消费唯一 worker owner

**Files:**
- Modify: `agent/composition/src/audit.rs`
- Modify: `agent/composition/src/runtime.rs`
- Modify: `agent/composition/src/app.rs` only if ownership transfer requires an explicit local binding
- Modify: `agent/composition/tests/audit_worker_assembly.rs`
- Modify: `agent/composition/src/app.rs` inline test `successful_runtime_invocation_persists_versioned_session_usage_jsonl`
- Test: `agent/composition/tests/audit_worker_assembly.rs`
- Test: `agent/composition/src/app.rs`

- [ ] **Step 1: 先把 Composition 测试改为消费式 API**

在 `audit_worker_assembly.rs`：

- `handle` 重命名为 `worker`；
- 所有 `shutdown().await` outcome 断言改为 `worker.shutdown().await;`；
- `wire_session_audit` 测试将 `session_audit.shutdown().await` 改为消费调用；
- 删除 `UsageShutdownOutcome` 引用。

把 production 测试扩为同一 worker 写两个 Session：

```rust
let first = usage_record("01900000-0000-7000-8000-000000000011", "first");
let second = usage_record("01900000-0000-7000-8000-000000000021", "second");
assert_eq!(sink.try_record(first.clone()), UsageEmitOutcome::Accepted);
assert_eq!(sink.try_record(second.clone()), UsageEmitOutcome::Accepted);
session_audit.shutdown().await;
```

通过 `UsageQuery { session_id: None, ... }` 查询，并断言结果同时包含 `first`、`second`，summary 的 record count/token sums 为两条总和。

若当前文件只有内联构造，先提取仅供测试的 `usage_record(session_id, id)` helper，不新增生产 fixture。

- [ ] **Step 2: 运行 Composition RED**

Run:

```bash
cargo test -p composition --test audit_worker_assembly -- --nocapture
```

Expected: FAIL to compile because `SessionAudit::shutdown` 仍借用 `&self` 并返回退役 outcome，且 `AuditWorkerAssembly` 仍暴露 `handle`。

- [ ] **Step 3: 收窄 Composition Audit owner**

`agent/composition/src/audit.rs` 使用：

```rust
use audit::{
    file_usage_append_store, start_usage_worker, UsageSender, UsageWorker, UsageWorkerConfig,
};

pub struct AuditWorkerAssembly {
    pub sender: UsageSender,
    pub worker: UsageWorker,
}

pub struct SessionAudit {
    sink: std::sync::Arc<dyn runtime::UsageSink>,
    worker: UsageWorker,
}

impl SessionAudit {
    pub fn usage_sink(&self) -> std::sync::Arc<dyn runtime::UsageSink> {
        std::sync::Arc::clone(&self.sink)
    }

    pub async fn shutdown(self) {
        self.worker.shutdown().await;
    }
}
```

`wire_session_audit` 与 `wire_audit_worker` 对应使用 `worker` 字段；函数参数保持 `agents_dir + snapshot`，不增加 Session 参数，因为 worker 必须支持运行中 Session 切换并按 record 分区。

- [ ] **Step 4: 修改 Runtime 场景测试的 owner 移动**

`successful_runtime_invocation_persists_versioned_session_usage_jsonl` 中将 assembly 声明为 mutable：

```rust
let mut assembly = crate::runtime::from_args_with_gateways(
    args,
    gateways,
    workspace,
    config,
    &agents_dir,
)
.await
.expect("runtime assembly");
```

收尾改为：

```rust
assembly
    .audit
    .take()
    .expect("session audit")
    .shutdown()
    .await;
```

保留既有 Main/Sub 两条 record 都具有 canonical session ID 的断言。这一层证明 Runtime records 提供正确 Session，Audit worker 自身不保存 Session。

- [ ] **Step 5: 运行 Composition 与 Runtime 相邻边界**

Run:

```bash
cargo test -p composition --test audit_worker_assembly -- --nocapture
cargo test -p composition successful_runtime_invocation_persists_versioned_session_usage_jsonl -- --nocapture
```

Expected: PASS；同一 worker 分区两个 Session，全局 query 汇总；Runtime Main/Sub JSONL 仍为 canonical Session。

- [ ] **Step 6: 运行 Composition production clippy**

Run:

```bash
cargo clippy -p composition --lib -- -D warnings
```

Expected: PASS。

- [ ] **Step 7: 提交 Composition/Runtime 链路**

```bash
git add agent/composition/src/audit.rs \
  agent/composition/src/runtime.rs \
  agent/composition/src/app.rs \
  agent/composition/tests/audit_worker_assembly.rs
git commit -m "refactor(composition)!: #1596 消费唯一 Audit worker owner"
```

仅添加实际发生变化的文件；若 `runtime.rs` / `app.rs` 中某文件无需修改，不纳入 commit。

---

### Task 5: 让 CLI 移动并消费当前 Audit owner

**Files:**
- Modify: `apps/cli/src/chat.rs`
- Verify: `apps/cli/src/chat_tests.rs`

- [ ] **Step 1: 保持既有 frontend drain 行为测试为回归门禁**

确认以下三个测试仍存在且不改弱：

- `frontend_preserves_original_result_when_audit_drain_is_absent`
- `frontend_success_runs_audit_drain_once_and_preserves_success`
- `frontend_failure_runs_audit_drain_once_and_preserves_original_error`

它们已覆盖 helper 的结果优先级；本任务不新增 outcome fake，因为 outcome 已删除。

- [ ] **Step 2: 运行 CLI 基线测试**

Run:

```bash
cargo test -p cli chat::tests -- --nocapture
```

Expected: PASS before production ownership wiring changes。

- [ ] **Step 3: 从 mutable bootstrap 中 take Audit owner**

把 `run_chat` 中 bootstrap 声明改为 mutable：

```rust
let mut bootstrap = composition::app::build_agent_bootstrap(args.into())
    .await
    .unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        std::process::exit(1);
    });
```

quiet 分支在调用 helper 前取得 drain：

```rust
let audit_drain = bootstrap
    .session_audit
    .take()
    .map(|session_audit| async move { session_audit.shutdown().await });
run_frontend_with_audit_drain(client, audit_drain, move |client| async move {
    crate::chat::no_tui::run_no_tui_chat(client, quiet_session_id, command_router).await
})
.await
```

TUI 分支同样在调用 helper 前 take：

```rust
let audit_drain = bootstrap
    .session_audit
    .take()
    .map(|session_audit| async move { session_audit.shutdown().await });
run_frontend_with_audit_drain(
    client,
    audit_drain,
    move |client| async move { app.run(client).await },
)
.await
```

不再使用 `bootstrap.session_audit.as_ref()`，确保生产调用点不能重复 shutdown。

- [ ] **Step 4: 运行 CLI 测试与 clippy**

Run:

```bash
cargo test -p cli chat::tests -- --nocapture
cargo clippy -p cli --bin aemeath -- -D warnings
```

Expected: 既有 3 个 drain 测试及其他 chat tests PASS，production clippy PASS。

- [ ] **Step 5: 提交 CLI 所有权变更**

```bash
git add apps/cli/src/chat.rs
git commit -m "refactor(cli)!: #1596 消费 Audit drain owner"
```

---

### Task 6: 清理 façade、设计文档和测试矩阵

**Files:**
- Modify: `.agents/hooks/check-crate-api-boundary.sh`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
- Modify: `docs/design/02-modules/audit/README.md`
- Modify: `docs/design/02-modules/audit/01-usage-storage.md`
- Verify: `docs/superpowers/specs/2026-08-24-audit-session-jsonl-only-design.md`

- [ ] **Step 1: 更新 crate-root façade 白名单**

在 Audit allow set 中删除：

```text
UsagePipelineMetricsSnapshot
UsageShutdownOutcome
UsageWorkerHandle
```

增加：

```text
UsageWorker
```

保留 `UsageSender`、`UsageWorkerConfig`、`start_usage_worker`、`usage_query_service` 与全部 JSONL/query PL。

- [ ] **Step 2: 更新架构守卫人类可读索引**

将 `docs/design/03-engineering/01-architecture-guards.md` 的 Audit 描述改为：

```text
Audit 登记 Usage/Query PL、AppendLog 入口、无指标 `UsageSender`、消费式 `UsageWorker` 及被 Composition 消费的 `usage_query_service` 查询装配入口；退役 worker metrics 与共享 shutdown outcome 不得重新进入 façade。
```

- [ ] **Step 3: 重写 Audit README 的 worker 与 shutdown 章节**

必须删除所有以下 Current 声明：

- 一致 state metrics；
- cumulative threshold warning；
- `accepted_total` / `completed_total` / `write_failed_total` / `drain_abandoned_total`；
- `Running/ShuttingDown/Stopped`；
- 重复 shutdown completion；
- `TimedOut { unconfirmed }`。

替换为已批准设计中的明确行为：

- `try_record` 只有短 channel-slot 临界区；
- worker 按每条 record 的 Session 分区；
- Session 切换无需重建 worker；
- `UsageWorker::shutdown(self)` 消费 owner；
- timeout/取消/Drop abort；
- JSONL 是唯一事实；
- 全局 UsageQuery 即时重算；
- append/flush/timeout 只写不含原文的诊断 warning。

不要引用 Issue/PR 编号；修改历史使用稳定领域描述，不新增外部编号。

- [ ] **Step 4: 更新 Usage Storage Current 状态和测试矩阵**

在 `01-usage-storage.md`：

- 将“worker 写失败增加指标”改为“worker 写失败仅记录诊断并继续，不保存累计状态”；
- 将 worker 行从“指标、精确 timeout、幂等 shutdown”改为“跨 Session record 分区、失败隔离、消费式 drain、timeout/取消 abort”；
- 将 Composition 行明确为“一 frontend/runtime worker，Main/Sub 共用 sender，records 按 canonical Session 分区”；
- 保留 query 全局跨分区分页与 summary；
- 更新测试数量和覆盖率只能使用本轮实际验证结果，实施尚未运行 coverage 前不得预填百分比；
- 增加变更历史，使用稳定领域术语而不是 Issue 编号。

- [ ] **Step 5: 扫描退役概念**

Run:

```bash
rg -n "UsagePipelineMetricsSnapshot|UsageShutdownOutcome|UsageWorkerHandle|accepted_total|completed_total|write_failed_total|drain_abandoned_total|TimedOut \{ unconfirmed|重复 shutdown" \
  agent/features/audit agent/composition apps/cli \
  docs/design/02-modules/audit \
  .agents/hooks/check-crate-api-boundary.sh \
  docs/design/03-engineering/01-architecture-guards.md
```

Expected: zero production/test/Current-design matches。若修改历史保留旧事实，必须明确标注为已退役历史，且不能被 Current 章节引用。

- [ ] **Step 6: 运行 façade 与文档相关守卫**

Run:

```bash
.agents/hooks/check-crate-api-boundary.sh
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh --fast
```

Expected: PASS。

- [ ] **Step 7: 提交 Guard 与设计同步**

```bash
git add .agents/hooks/check-crate-api-boundary.sh \
  docs/design/03-engineering/01-architecture-guards.md \
  docs/design/02-modules/audit/README.md \
  docs/design/02-modules/audit/01-usage-storage.md
git commit -m "docs(audit): #1596 回写 JSONL 单一事实边界"
```

---

### Task 7: 定向验证并清理废弃路径

**Files:**
- Verify all changed files
- Modify only files containing stale references discovered by the scans

- [ ] **Step 1: 格式化**

Run:

```bash
cargo fmt --all
cargo fmt --all --check
```

Expected: PASS。

- [ ] **Step 2: 运行 Audit 全目标测试**

Run:

```bash
cargo test -p audit --all-targets
```

Expected: all Audit unit/contract tests PASS。

- [ ] **Step 3: 运行每层相邻边界测试**

Run:

```bash
cargo test -p runtime invocation_usage -- --nocapture
cargo test -p composition --test audit_worker_assembly -- --nocapture
cargo test -p composition successful_runtime_invocation_persists_versioned_session_usage_jsonl -- --nocapture
cargo test -p cli chat::tests -- --nocapture
```

Expected: all PASS；Runtime drop 不改写结果、Composition 跨 Session JSONL、Runtime canonical Session、CLI frontend/drain 正交分别有证据。

- [ ] **Step 4: 运行受影响 crate production checks**

Run:

```bash
cargo check -p audit --lib
cargo clippy -p audit --lib -- -D warnings
cargo check -p runtime --lib
cargo clippy -p runtime --lib -- -D warnings
cargo check -p composition --lib
cargo clippy -p composition --lib -- -D warnings
cargo check -p cli --bin aemeath
cargo clippy -p cli --bin aemeath -- -D warnings
```

Expected: all PASS。

- [ ] **Step 5: 运行受影响 crate all-targets clippy**

Run:

```bash
cargo clippy -p audit --all-targets -- -D warnings
cargo clippy -p runtime --all-targets -- -D warnings
cargo clippy -p composition --all-targets -- -D warnings
cargo clippy -p cli --all-targets -- -D warnings
```

Expected: all PASS。

- [ ] **Step 6: 扫描废弃 API 和多余测试**

Run:

```bash
rg -n "UsagePipelineMetricsSnapshot|UsageShutdownOutcome|UsageWorkerHandle|\.metrics\(\)|completion: Mutex|enum Lifecycle" .
rg -n "session.*worker registry|shutdown coordinator|global.*metrics|per.session.*metrics" \
  agent docs/design .agents
```

Expected: zero active production/test/Current-design matches。设计计划或历史记录中的讨论性文字不作为生产残留，但必须与“已删除”语义一致。

- [ ] **Step 7: 检查 diff 与工作区**

Run:

```bash
git diff --check
git status --short
git diff --stat main...HEAD
git diff main...HEAD -- agent/features/audit agent/composition apps/cli .agents/hooks/check-crate-api-boundary.sh docs/design/02-modules/audit docs/design/03-engineering/01-architecture-guards.md
```

Expected: 无 whitespace error；只包含 #1596 设计、实现、测试、Guard 和 Audit 文档范围。

- [ ] **Step 8: 若格式化或残留清理产生变更则提交**

```bash
git status --short
git add agent/features/audit agent/composition apps/cli \
  .agents/hooks/check-crate-api-boundary.sh \
  docs/design/02-modules/audit \
  docs/design/03-engineering/01-architecture-guards.md
git diff --cached --check
git commit -m "chore(audit): #1596 清理退役 worker 状态"
```

只允许暂存 `git status --short` 中由格式化或退役引用清理产生的文件；若命令列出的路径没有未提交变更，记录“无需 cleanup commit”。

---

### Task 8: 全量门禁、覆盖率和 GitHub 验收

**Files:**
- Modify: `docs/design/02-modules/audit/01-usage-storage.md` with actual verification evidence
- GitHub: Issue #1596

- [ ] **Step 1: 运行 workspace tests**

Run:

```bash
cargo test --workspace
```

Expected: PASS, zero failed。

- [ ] **Step 2: 运行 workspace all-targets clippy**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS, zero warnings。

- [ ] **Step 3: 运行完整架构守卫**

Run:

```bash
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh --full
```

Expected: `All full architecture guards passed.`

- [ ] **Step 4: 运行 coverage gate**

Run:

```bash
scripts/coverage.sh
```

Expected: PASS。记录 workspace 与 Audit 的 region/function/line 实际值；不得沿用 #1579 的旧值。

- [ ] **Step 5: 回写实际验证证据**

在 `docs/design/02-modules/audit/01-usage-storage.md` 记录：

- 定向测试名称与结果；
- workspace test/clippy；
- production checks；
- 完整 architecture guards；
- coverage gate 和实际百分比；
- 首次 RED 失败原因；
- 无剩余 metrics/shutdown outcome/coordinator；
- JSONL-only 与全局 UsageQuery 验收结论。

- [ ] **Step 6: 提交验收证据**

```bash
git add docs/design/02-modules/audit/01-usage-storage.md
git commit -m "docs(audit): #1596 记录 JSONL-only 最终门禁"
```

- [ ] **Step 7: 重新运行受文档提交影响的最小门禁**

Run:

```bash
cargo fmt --all --check
git diff --check main...HEAD
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh --fast
```

Expected: PASS。

- [ ] **Step 8: 更新 Issue #1596 正文**

先生成 `/tmp/aemeath-issue-1596-body.md`，内容为最终批准边界、实际验证命令/结果和完成后的 checklist；随后执行：

```bash
gh issue edit 1596 --repo rushsinging/aemeath \
  --body-file /tmp/aemeath-issue-1596-body.md
```

正文必须明确：

- JSONL 是唯一事实；
- worker 无状态按 record Session 分区；
- 删除 metrics/outcome/coordinator；
- shutdown 消费 owner；
- UsageQuery 保持全局能力；
- 验证命令及结果。

- [ ] **Step 9: 刷新项目进度**

Run:

```bash
gh api graphql -f query='query { repository(owner:"rushsinging", name:"aemeath") { issue(number:857) { state subIssues(first:100) { totalCount nodes { number state } } } issue1596: issue(number:1596) { state parent { number state } milestone { number title } } } }'
gh api repos/rushsinging/aemeath/milestones/2 --jq '{open_issues,closed_issues,total:(.open_issues+.closed_issues)}'
```

记录 #857 已完成/总子项、#1596 状态和 milestone 完成比例。

---

### Task 9: 请求代码审查并创建 PR

**Files:**
- Review all changes from `main...HEAD`
- GitHub: PR targeting `main`

- [ ] **Step 1: 请求独立代码审查**

使用 `superpowers:requesting-code-review`，向 reviewer 提供：

- Base SHA：worktree 创建时的 `main`；
- Head SHA：当前 branch HEAD；
- 设计：`docs/superpowers/specs/2026-08-24-audit-session-jsonl-only-design.md`；
- 要求重点核对消费式 shutdown 的取消安全、Session 切换、全局 query、Drop abort 和退役 API 清理。

Critical/Important finding 必须在创建 PR 前修复并重新验证。

- [ ] **Step 2: 执行最终验证**

Run:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh --full
git diff --check main...HEAD
git status --short --branch
```

Expected: all PASS；工作区 clean。

- [ ] **Step 3: 推送分支**

Run with one-hour timeout because pre-push runs full hooks and tests:

```bash
git push -u origin fix/1596-audit-session-jsonl-only
```

Expected: pre-push PASS，remote branch created。

- [ ] **Step 4: 创建 PR**

PR 标题：

```text
refactor(audit)!: #1596 仅保留 Usage JSONL 事实
```

PR body 必须包括：

- JSONL-only 设计摘要；
- 删除的 metrics/outcome/coordinator API；
- 消费式 shutdown 与取消/Drop abort；
- Session 切换与全局 UsageQuery 保持；
- TDD RED 证据；
- 全量验证与覆盖率；
- `Closes #1596`；
- `Refs #857`。

优先使用 `gh api repos/rushsinging/aemeath/pulls` 或 body file，避免短 timeout。

- [ ] **Step 5: 核对远端状态**

Run:

```bash
pr_number=$(gh pr list --repo rushsinging/aemeath \
  --head fix/1596-audit-session-jsonl-only \
  --json number --jq '.[0].number')
test -n "$pr_number"
gh pr view "$pr_number" --repo rushsinging/aemeath \
  --json number,url,state,isDraft,mergeable,mergeStateStatus,statusCheckRollup,closingIssuesReferences,files,commits
gh issue view 1596 --repo rushsinging/aemeath --json state,body,url,milestone
gh issue view 857 --repo rushsinging/aemeath --json state,body,url
```

Expected: PR open，#1596 linked for closure，#857 remains open until parent governance completes；报告 checks 的真实状态。
