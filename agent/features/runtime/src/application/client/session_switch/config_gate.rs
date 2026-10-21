//! Gate-aware config façade（#2025 迁移自 context）：config 读写的
//! session-switch 竞态防护，属切换协议配套（shared 读 / exclusive 写）。

use std::sync::{Arc, RwLock as StdRwLock};

use async_trait::async_trait;
use config::{
    ConfigChangeData, ConfigReader, ConfigSubscriptionData, ConfigUpdateData, ConfigWriter,
    PreparedConfigUpdateData, ProjectConfigParticipant,
};
use memory::api::{MemoryOpener, MemoryPort, ProjectMemoryKey};
use project::WorkspaceReader;
use share::config::domain::snapshot::ConfigSnapshot;

use super::coordinator::SessionSwitchCoordinator;
use super::gate::SessionSwitchGate;
use crate::LOG_TARGET;

impl SessionSwitchCoordinator {
    /// 协议配套：shared permit 内捕获 config snapshot 的 reader façade。
    pub fn gate_aware_config_reader(&self) -> Arc<dyn ConfigReader> {
        Arc::new(GateAwareConfigReader {
            gate: self.gate.clone(),
            config_reader: Arc::clone(&self.config_reader),
        })
    }

    /// 协议配套：exclusive 区写 config 并联动 committed memory 的 writer façade。
    pub fn gate_aware_config_writer(&self) -> Arc<dyn ConfigWriter> {
        Arc::new(GateAwareConfigWriter {
            gate: self.gate.clone(),
            config_participant: Arc::clone(&self.config_participant),
            workspace_read: Arc::clone(&self.workspace_read),
            memory_opener: self.memory_opener.clone(),
            committed_memory: Arc::clone(&self.committed_memory),
        })
    }
}

/// Gate-aware [`ConfigReader`] façade。
///
/// `snapshot` / `subscribe` 先取 **shared** session-switch permit 再读，
/// 保证快照捕获时无 resume 进行中；permit 读后即释放，返回的
/// `watch::Receiver` 在 permit 释放后继续接收更新。
pub struct GateAwareConfigReader {
    gate: SessionSwitchGate,
    config_reader: Arc<dyn ConfigReader>,
}

#[async_trait]
impl ConfigReader for GateAwareConfigReader {
    // 同步委托：async gate 只能覆盖 async 入口；同步读经 gate-aware writer 的
    // 互斥已由会话切换协议保证。
    fn committed_snapshot(&self) -> ConfigSnapshot {
        self.config_reader.committed_snapshot()
    }

    fn subscribe_committed(&self) -> tokio::sync::watch::Receiver<ConfigSnapshot> {
        self.config_reader.subscribe_committed()
    }

    async fn refresh_if_sources_changed(
        &self,
    ) -> Result<config::ConfigRefreshOutcomeData, share::error::DomainError> {
        // Err 原样透传：刷新失败的降级语义由最外层消费方（runtime turn 边界）判定。
        self.config_reader.refresh_if_sources_changed().await
    }

    async fn snapshot(&self) -> Result<ConfigSnapshot, share::error::DomainError> {
        let _permit =
            self.gate.acquire_shared().await.map_err(|_| {
                share::error::DomainError::unavailable("config", "配置读取暂不可用")
            })?;
        Ok(self.config_reader.committed_snapshot())
    }

    async fn subscribe(&self) -> Result<ConfigSubscriptionData, share::error::DomainError> {
        let _permit =
            self.gate.acquire_shared().await.map_err(|_| {
                share::error::DomainError::unavailable("config", "配置读取暂不可用")
            })?;
        let changes = self.config_reader.subscribe_committed();
        let initial = changes.borrow().clone();
        Ok(ConfigSubscriptionData { initial, changes })
    }
}

/// Gate-aware [`ConfigWriter`] façade。
///
/// Writer 流水线：
///
/// 1. **取 exclusive permit** —— 阻塞全部 shared 绑定与 resume 直到更新落定。
/// 2. **Config prepare_update** —— 产出 [`PreparedConfigUpdateData`]，不提交。
/// 3. **Eager open 候选 Memory** —— 基于当前 committed workspace 身份与候选
///    `MemoryConfig`。
/// 4. **Spawn 临界区** —— permit + prepared update + 候选 Memory 移入
///    `tokio::spawn` owned 任务执行 `persist_update`：
///    - **Err** —— 旧 Memory 与 Config 原样保留；任务返回
///      `Err(storage("config", "配置持久化失败：…"))`。
///    - **Ok** —— 候选 Memory 装入 committed holder，随后 `commit_update`
///      **最后**触发 config watch。config commit 警告只记日志不转错。
///
/// `update()` future await 该 JoinHandle。执行到持久化交接
/// （`tokio::spawn`）后，取消 / drop 外层 future 只停止等待：owned 任务
/// 继续跑完并在提交成功后安装 Memory 与 Config。
pub struct GateAwareConfigWriter {
    gate: SessionSwitchGate,
    config_participant: Arc<dyn ProjectConfigParticipant>,
    workspace_read: Arc<dyn WorkspaceReader>,
    memory_opener: Box<dyn MemoryOpener>,
    committed_memory: Arc<StdRwLock<Arc<dyn MemoryPort>>>,
}

#[async_trait]
impl ConfigWriter for GateAwareConfigWriter {
    async fn update(
        &self,
        command: ConfigUpdateData,
    ) -> Result<ConfigChangeData, share::error::DomainError> {
        // 1. 取 owned exclusive permit。
        let permit = self.gate.acquire_owned_exclusive().await.map_err(|_| {
            share::error::DomainError::invalid("config", "session switch gate closed")
        })?;

        // 2. Config prepare_update（不提交）。
        let prepared: PreparedConfigUpdateData =
            self.config_participant.prepare_update(command).await?;

        // 3. 从当前 committed workspace 身份推导 memory key。
        let identity = self.workspace_read.project_identity();
        let candidate_memory_config = prepared.memory_config().clone();
        let memory_key =
            ProjectMemoryKey::derive(&identity.initial_cwd, identity.git_common_dir.as_deref())
                .map_err(|e| {
                    share::error::DomainError::invalid(
                        "config",
                        format!("memory key derivation: {e}"),
                    )
                })?;

        // 4. Eager open 候选 memory。
        let candidate_memory: Arc<dyn MemoryPort> = self
            .memory_opener
            .open_memory(&memory_key, &candidate_memory_config)
            .await
            .map_err(|e| {
                share::error::DomainError::invalid("config", format!("memory open: {e}"))
            })?;

        // 5. Spawn owned 临界区：persist_update + 条件 commit。
        let config_participant = Arc::clone(&self.config_participant);
        let committed_memory = Arc::clone(&self.committed_memory);

        let handle = tokio::spawn(async move {
            // 整个临界区持 exclusive permit。
            let _permit = permit;

            // 持久化失败时旧 Memory 与 Config 原样保留。
            let ready = config_participant
                .persist_update(prepared)
                .await
                .map_err(|err| {
                    share::error::DomainError::storage("config", format!("配置持久化失败：{err}"))
                })?;

            // 警告仅记录——不转错误。
            if let Some(warning) = ready.warning() {
                log::warn!(target: LOG_TARGET, "config commit warning: {warning:?}");
            }
            // 提交 config 前先安装新 Memory。
            *committed_memory.write().unwrap() = candidate_memory;
            // commit_update 最后触发 watch。
            let change_set = config_participant.commit_update(*ready);
            Ok(change_set)
            // _permit 此处 drop → exclusive 锁释放。
        });

        // Await spawned 任务。交接后 drop 外层 future 只 drop JoinHandle；
        // owned 任务继续执行。
        match handle.await {
            Ok(result) => result,
            Err(join_err) => Err(share::error::DomainError::invalid(
                "config",
                format!("background config task: {join_err}"),
            )),
        }
    }
}
