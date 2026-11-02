//! 评分、校准与模型资产端口：消费方只依赖端口，NEVER 直接感知引擎型号。

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::domain::{
    CalibrationLevel, ModelAsset, ModelManifest, ModelManifestError, ScoringAnswer,
    ScoringQuestion, ScoringState, ScoringUnavailable,
};

/// 评分端口：批量评分；服务不可用返回 `ScoringUnavailable`（消费点据此回退）。
#[async_trait]
pub trait ScoringPort: Send + Sync {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable>;
}

/// 一条「评分 + 后续观测标签」记录，供离线/在线校准拟合。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalibrationObservation {
    pub question: ScoringQuestion,
    pub probabilities: Vec<f64>,
    pub label: String,
    pub engine_revision: String,
    pub timestamp: String,
}

/// 校准端口：observe 回路落盘 + 当前生效校准 artifact。
#[async_trait]
pub trait CalibrationPort: Send + Sync {
    /// 落盘一条观测记录，供校准拟合。
    async fn observe(&self, record: CalibrationObservation);

    /// 当前生效的校准级别；无 artifact 时为 `CalibrationLevel::Raw`。
    fn current(&self) -> CalibrationLevel;
}

/// 已通过全部校验的模型资产：manifest 单一来源 + 安装根目录。
///
/// 字段私有，构造器收窄为 `pub(crate)`：外部 NEVER 可凭字段或构造器自行装配，
/// 只能消费先校验 manifest 的实例，并经只读 getters 读取。
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledAssets {
    /// 校验通过的 manifest（路径、URL、字节数、SHA-256 的唯一声明处）。
    manifest: ModelManifest,
    /// 已安装 revision 的根目录。
    install_root: PathBuf,
}

impl InstalledAssets {
    /// 装配入口（`pub(crate)`）：先全量校验 manifest（`validate()`，非法/空资产即拒绝），通过才装配。
    ///
    /// 仅本 crate 的存储 adapter 与测试经此装配；crate 外只有只读 getters
    /// （[`Self::manifest`] / [`Self::install_root`]），NEVER 暴露可拼装的构造能力。
    pub(crate) fn new(
        manifest: ModelManifest,
        install_root: PathBuf,
    ) -> Result<Self, ModelManifestError> {
        manifest.validate()?;
        Ok(Self {
            manifest,
            install_root,
        })
    }

    /// 校验通过的 manifest（只读）。
    pub fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    /// 已安装 revision 的根目录（只读）。
    pub fn install_root(&self) -> &std::path::Path {
        &self.install_root
    }
}

/// 已安装但校验失败的原因类别：损坏、不可读、契约不支持。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidAssetKind {
    /// 内容损坏（长度 / SHA-256 与 manifest 不符）。
    Corrupt,
    /// 文件系统不可读（权限、IO 错误）。
    Unreadable,
    /// 契约不支持（schema 版本、维度或布局不符）。
    Unsupported,
}

impl std::fmt::Display for InvalidAssetKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt => write!(formatter, "模型资产已损坏"),
            Self::Unreadable => write!(formatter, "模型资产不可读"),
            Self::Unsupported => write!(formatter, "模型资产不受支持"),
        }
    }
}

/// 模型资产三态：fail-closed —— 只有 `Installed` 可加载，缺失与损坏分开报告。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelAssetState {
    /// 资产齐备且逐项校验通过。
    Installed(InstalledAssets),
    /// 本地未安装（提示用户执行手动下载命令）。
    Missing,
    /// 已安装但校验失败：typed 原因类别 + 中文 detail。
    Invalid {
        /// 失败类别（损坏 / 不可读 / 不支持）。
        kind: InvalidAssetKind,
        /// 中文失败原因。
        detail: String,
    },
}

/// 模型资产端口：只暴露「安装状态解析」一个职责。
///
/// 文件系统遍历、长度/SHA-256 计算与原子安装细节 MUST 留在 adapter 内；
/// 端口 NEVER 暴露下载、IO 或哈希操作。
#[async_trait]
pub trait ModelAssetPort: Send + Sync {
    /// 解析本地模型资产安装状态：齐备 → `Installed`，未安装 → `Missing`，
    /// 校验失败 → `Invalid`（永不把半成品识别为已安装）。
    async fn installed_assets(&self) -> ModelAssetState;
}

/// 资产抓取失败类别：typed kind —— application 据此映射下载结果，NEVER 字符串匹配决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactFetchErrorKind {
    /// 网络传输失败（连接、超时、请求发送或响应流读取中断）。
    Network,
    /// 来源地址不安全（非 https、重定向降级、跳数超限或 URL 非法）。
    UnsafeSource,
    /// 服务端返回非 2xx 状态。
    HttpStatus,
    /// 响应长度与 manifest 字节数不符（Content-Length 预检、流累计越界或结束不足）。
    LengthMismatch,
    /// 目标写入被拒绝（已存在、路径含符号链接、父目录异常或 IO 失败）。
    DestinationRejected,
}

impl std::fmt::Display for ArtifactFetchErrorKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network => write!(formatter, "网络传输失败"),
            Self::UnsafeSource => write!(formatter, "来源地址不安全"),
            Self::HttpStatus => write!(formatter, "服务端状态异常"),
            Self::LengthMismatch => write!(formatter, "响应长度与 manifest 不符"),
            Self::DestinationRejected => write!(formatter, "目标写入被拒绝"),
        }
    }
}

/// 资产抓取失败（typed kind + 中文 detail）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactFetchError {
    /// 失败类别。
    pub kind: ArtifactFetchErrorKind,
    /// 中文失败原因。
    pub detail: String,
}

impl std::fmt::Display for ArtifactFetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}：{}", self.kind, self.detail)
    }
}

impl std::error::Error for ArtifactFetchError {}

/// 工件抓取端口：把**已校验** [`ModelAsset`] 的内容流式写入 staging 根下对应相对路径。
///
/// - 实现 MUST NEVER 把整份响应一次性读入内存（GGUF 约 775MB）：
///   `bytes_stream()` 分块 + 增量写入目标文件。
/// - 目标路径只能由 `staging_root` + `asset.path` 推导；MUST 拒绝覆盖既有目标、
///   拒绝末段与中间目录符号链接，并保证结束字节数恰好等于 `asset.byte_length`。
/// - 任何失败以 [`ArtifactFetchError`] typed 报告；application 失败路径随后
///   经 [`ModelInstallerPort::discard_staging`] 显式清理整个 staging。
/// - 实现失败时 MUST 尽力删除自己写下的半成品目标文件；其父目录与整个
///   staging 目录由安装端 [`ModelInstallerPort::discard_staging`] **整体**清理
///   （application NEVER 逐路径删除，也 NEVER 感知半成品细节）。
#[async_trait]
pub trait ArtifactFetcherPort: Send + Sync {
    /// 将资产流式写入 `staging_root` 下的 `asset.path` 相对位置。
    async fn fetch_asset_into_staging(
        &self,
        staging_root: &Path,
        asset: &ModelAsset,
    ) -> Result<(), ArtifactFetchError>;
}

/// 安装端口失败类别：typed kind —— application 据此映射，NEVER 字符串匹配决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelInstallPortErrorKind {
    /// 暂存区创建或准入被拒（安装根形态异常、暂存命名冲突）。
    StagingUnavailable,
    /// 暂存区绑定核对被拒（owner root / owner revision / 路径 / 全名与当前存储不符）：
    /// typed 拒绝，被拒路径 **NEVER** 被当前存储删除。
    StagingRejected,
    /// 暂存区显式清理失败。
    StagingDiscardFailed,
    /// 暂存内容校验失败（长度 / SHA-256 / canonical manifest / 布局）。
    VerificationFailed,
    /// 安装目标已存在且无效或提交冲突：拒绝覆盖。
    InstallConflict,
    /// 安装原子提交 IO 失败。
    InstallIoFailed,
    /// 后台阻塞任务执行失败。
    TaskFailed,
}

impl std::fmt::Display for ModelInstallPortErrorKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StagingUnavailable => write!(formatter, "暂存目录不可用"),
            Self::StagingRejected => write!(formatter, "暂存区绑定核对被拒"),
            Self::StagingDiscardFailed => write!(formatter, "暂存目录清理失败"),
            Self::VerificationFailed => write!(formatter, "暂存内容校验失败"),
            Self::InstallConflict => write!(formatter, "安装目标冲突"),
            Self::InstallIoFailed => write!(formatter, "安装提交 IO 失败"),
            Self::TaskFailed => write!(formatter, "安装任务执行失败"),
        }
    }
}

/// 安装端口失败（typed kind + 中文 detail）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInstallPortError {
    /// 失败类别。
    pub kind: ModelInstallPortErrorKind,
    /// 中文失败原因。
    pub detail: String,
}

impl std::fmt::Display for ModelInstallPortError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}：{}", self.kind, self.detail)
    }
}

impl std::error::Error for ModelInstallPortError {}

/// 暂存安装结果：本次原子提交落地，或 final 已是有效安装（并发竞争者，幂等保留）。
#[derive(Debug)]
pub enum StagedInstallOutcome {
    /// 本次暂存已原子 no-replace 提交为 final revision。
    Installed(InstalledAssets),
    /// final revision 已存在且校验有效：MUST NOT 覆盖，幂等返回现有安装。
    AlreadyValid(InstalledAssets),
}

/// 模型安装暂存区句柄（端口层 token）：安装根下的隐藏暂存目录。
///
/// 字段私有，装配入口 `pub(crate)`：只有本 crate 的安装 adapter 能创建，
/// 并在创建时把 **owner 绑定**（owner root + owner revision + path）写入 token；
/// application 侧只可见 [`path`](Self::path) 一个只读 getter，NEVER 能读取、
/// 修改绑定字段，也 NEVER 能自行装配 token。
///
/// 安装 / 清理入口（[`ModelInstallerPort::install_staged`] /
/// [`ModelInstallerPort::discard_staging`]）MUST 先核对绑定与存储一致、
/// 再复用暂存全名校验；不匹配 → typed [`ModelInstallPortErrorKind::StagingRejected`]
/// 且被拒路径 **NEVER** 被当前存储删除（token 在返回前解除清理责任）。
///
/// armed 时 [`Drop`] 做**同步 best-effort 兜底清理**——正常失败/成功路径 MUST
/// 经 [`ModelInstallerPort::discard_staging`] 或 [`ModelInstallerPort::install_staged`]
/// 显式异步清理，NEVER 只依赖 `Drop`。进程中断留下的 stale 暂存目录本轮不清理，
/// 后续由启动 / 下载前 sweep 承接（Drop 只是同步兜底，不承担该职责）。
#[derive(Debug)]
#[must_use]
pub struct ModelStagingArea {
    /// 暂存目录路径。
    path: PathBuf,
    /// owner 绑定：创建该暂存的存储安装根（adapter 构造时写入）。
    owner_root: PathBuf,
    /// owner 绑定：创建该暂存时期的期望 engine revision。
    owner_revision: String,
    /// 是否仍负有清理责任。
    armed: bool,
}

impl ModelStagingArea {
    /// 武装一个由安装 adapter 创建、确认自有的暂存目录（owner 绑定一并写入）。
    pub(crate) fn armed(path: PathBuf, owner_root: PathBuf, owner_revision: String) -> Self {
        Self {
            path,
            owner_root,
            owner_revision,
            armed: true,
        }
    }

    /// 暂存目录路径（抓取端口的写入根；只是数据，不做 IO）。
    /// 这是 application 唯一可见的访问器；owner 绑定字段对 application 不可见。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 安装 adapter 专用：核对 owner 绑定与期望的存储（root + revision + 路径是
    /// root 直接子目录）。不匹配时**先解除本 token 的清理责任再返回原因**
    /// （被拒路径 NEVER 被当前存储删除），匹配时原样交还 token。
    pub(crate) fn verify_owner_binding(
        mut self,
        expected_root: &Path,
        expected_revision: &str,
    ) -> Result<Self, String> {
        let mismatch = if self.owner_root != expected_root {
            Some(format!(
                "暂存区 owner root 与当前存储不符：owner={}，expected={}",
                self.owner_root.display(),
                expected_root.display()
            ))
        } else if self.owner_revision != expected_revision {
            Some(format!(
                "暂存区 owner revision 与当前期望不符：owner={}，expected={expected_revision}",
                self.owner_revision
            ))
        } else if self.path.parent() != Some(expected_root) {
            Some(format!(
                "暂存区路径不是 owner root 的直接子目录：{}",
                self.path.display()
            ))
        } else {
            None
        };
        match mismatch {
            Some(detail) => {
                self.armed = false; // 被拒 token：Drop 也不得删除该路径
                Err(detail)
            }
            None => Ok(self),
        }
    }

    /// 安装 adapter 专用：绑定核对通过但后续准入（全名 / root 形态）失败时，
    /// 解除清理责任并放弃 token —— 被拒路径 **NEVER** 被当前存储删除。
    pub(crate) fn reject_without_cleanup(mut self) {
        self.armed = false;
    }

    /// 移交路径并解除本句柄的清理责任（安装 / 清理端口实现消费时用）。
    pub(crate) fn into_path(mut self) -> PathBuf {
        self.armed = false;
        self.path.clone()
    }
}

impl Drop for ModelStagingArea {
    fn drop(&mut self) {
        if self.armed {
            discard_staged_directory(&self.path);
        }
    }
}

/// 删除暂存目录（目录不存在视为已清理，幂等成功）。
pub(crate) fn remove_staged_directory(staging_dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(staging_dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// 尽力同步清理暂存目录（RAII 兜底的唯一清理实现；安装 adapter 的共享清理点）。
///
/// 清理失败仅记录日志、不掩盖原始错误；目录不存在视为已清理。
pub(crate) fn discard_staged_directory(staging_dir: &Path) {
    if let Err(error) = remove_staged_directory(staging_dir) {
        log::warn!(
            target: crate::LOG_TARGET,
            "model_staging_cleanup_failed path={} error={error}",
            staging_dir.display()
        );
    }
}

/// 模型安装端口：暂存区创建 / 校验后原子提交 / 显式异步清理三个职责。
///
/// 实现 MUST 保证：`create_staging` 产出的 token 携带创建方的 owner 绑定
/// （application 只可见 [`ModelStagingArea::path`]）；`install_staged` 与
/// `discard_staging` 先核对绑定再复用暂存全名校验，不匹配 typed
/// [`ModelInstallPortErrorKind::StagingRejected`] 且被拒路径 NEVER 删除；
/// `install_staged` **消费** staging，失败时实现已自行清理暂存
/// （RAII `Drop` 只是兜底）；`discard_staging` 是 application 正常失败路径的
/// 显式清理点，整个删除在 `spawn_blocking` 内完成。
#[async_trait]
pub trait ModelInstallerPort: Send + Sync {
    /// 在安装根下创建唯一隐藏暂存区。
    async fn create_staging(&self) -> Result<ModelStagingArea, ModelInstallPortError>;

    /// 校验暂存内容（长度 / SHA-256 / canonical manifest）并原子 no-replace 提交。
    /// final 已有效时幂等返回 [`StagedInstallOutcome::AlreadyValid`]，NEVER 覆盖。
    async fn install_staged(
        &self,
        staging: ModelStagingArea,
    ) -> Result<StagedInstallOutcome, ModelInstallPortError>;

    /// 显式异步清理暂存区（正常失败路径的常规清理点，NEVER 只依赖 `Drop`）。
    async fn discard_staging(&self, staging: ModelStagingArea)
        -> Result<(), ModelInstallPortError>;
}

#[cfg(test)]
#[path = "ports_tests.rs"]
mod tests;
