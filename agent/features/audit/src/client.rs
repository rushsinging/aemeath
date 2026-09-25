//! AuditClient：audit 的唯一对外角色（读写合一 + worker 生命周期）。
//!
//! 命名语法（façade SOP）：`AuditClient` 为 `<Domain><Role>` 角色形态；
//! `Usage*` 家族为数据契约（无 Role 词）；`wire_audit_*` 为装配工厂。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::adapters::query::UsageQueryService;
use crate::domain::{
    UsageDropReasonData, UsageEmitOutcomeData, UsagePageData, UsageQueryData, UsageRecordData,
};
use crate::ports::UsageAppendStorePort;

/// audit 存储句柄：内部 SPI（`UsageAppendStorePort`）不出签名的轻量包装。
#[derive(Clone)]
pub struct AuditStore {
    port: Arc<dyn UsageAppendStorePort>,
}

impl AuditStore {
    pub(crate) fn port(&self) -> Arc<dyn UsageAppendStorePort> {
        Arc::clone(&self.port)
    }
}

/// 装配工厂：文件系统审计存储。
pub fn wire_audit_store(port: Arc<dyn UsageAppendStorePort>) -> AuditStore {
    AuditStore { port }
}

type SenderSlot = Arc<Mutex<Option<mpsc::Sender<UsageRecordData>>>>;

/// 写角色：记录用量事实 + worker 管道生命周期。
///
/// Clone 语义：克隆共享发送端（多持有者并发记录）；worker join 归首个
/// 实例所有，`shutdown` 由持有完整实例的一侧调用。
pub struct AuditWriter {
    sender: SenderSlot,
    worker: Mutex<Option<crate::application::UsageWorkerHandle>>,
    timeout: Duration,
}

impl Clone for AuditWriter {
    fn clone(&self) -> Self {
        Self {
            sender: Arc::clone(&self.sender),
            worker: Mutex::new(None),
            timeout: self.timeout,
        }
    }
}

impl AuditWriter {
    pub(crate) fn from_parts(
        sender: SenderSlot,
        worker: crate::application::UsageWorkerHandle,
        timeout: Duration,
    ) -> Self {
        Self {
            sender,
            worker: Mutex::new(Some(worker)),
            timeout,
        }
    }

    /// 写：尽力记录一条用量事实（队列满/worker 停止时丢弃并说明原因）。
    pub fn try_record(&self, record: UsageRecordData) -> UsageEmitOutcomeData {
        let Ok(sender_slot) = self.sender.lock() else {
            return UsageEmitOutcomeData::Dropped(UsageDropReasonData::WorkerUnavailable);
        };
        let Some(sender) = sender_slot.as_ref() else {
            return UsageEmitOutcomeData::Dropped(UsageDropReasonData::WorkerUnavailable);
        };
        match sender.try_send(record) {
            Ok(()) => UsageEmitOutcomeData::Accepted,
            Err(mpsc::error::TrySendError::Full(_)) => {
                UsageEmitOutcomeData::Dropped(UsageDropReasonData::QueueFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                drop(sender_slot);
                if let Ok(mut slot) = self.sender.lock() {
                    *slot = None;
                }
                UsageEmitOutcomeData::Dropped(UsageDropReasonData::WorkerUnavailable)
            }
        }
    }

    /// worker 生命周期：关闭发送端并等待落盘完成（超时中止）。
    pub async fn shutdown(&self) {
        // 先取出 worker（guard 不跨 await），再等待。
        let worker = self.worker.lock().ok().and_then(|mut slot| slot.take());
        if let Some(worker) = worker {
            worker.shutdown(self.timeout).await;
        }
        if let Ok(mut sender_slot) = self.sender.lock() {
            *sender_slot = None;
        }
    }
}

/// 读角色：纯查询（无管道生命周期职责）。
#[derive(Clone)]
pub struct AuditReader {
    query: UsageQueryService,
}

impl AuditReader {
    /// 读：分页查询用量（粗分类边界错误）。
    pub async fn query_page(
        &self,
        query: UsageQueryData,
    ) -> Result<UsagePageData, share::error::DomainError> {
        self.query.query_page(query).await
    }
}

/// 装配工厂：审计读写角色（一次装配产出配套 Writer+Reader，writer 拥有 worker 管道）。
pub fn wire_audit_client(
    store: &AuditStore,
    capacity: usize,
    shutdown_timeout: Duration,
) -> (AuditWriter, AuditReader) {
    let (sender, receiver) = mpsc::channel(capacity.max(1));
    let sender: SenderSlot = Arc::new(Mutex::new(Some(sender)));
    let join = tokio::spawn(crate::application::run_usage_worker(receiver, store.port()));
    let writer = AuditWriter::from_parts(
        Arc::clone(&sender),
        crate::application::UsageWorkerHandle::new(join),
        shutdown_timeout,
    );
    let reader = AuditReader {
        query: UsageQueryService::from_store(store.port()),
    };
    (writer, reader)
}
