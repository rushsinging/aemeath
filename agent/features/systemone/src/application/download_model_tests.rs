//! `ModelDownloadService` 用例测试：fake 抓取端口 + 真实 `LocalModelAssetStore`
//! 覆盖幂等、fail-closed、顺序/相对布局、显式暂存清理与退出码映射。
//!
//! 每个用例独立 TempDir；夹具为小字节资产，NEVER 访问网络（抓取全部由 fake 脚本完成）。

use super::{DownloadOutcome, ModelDownloadErrorKind, ModelDownloadService};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::constants::{MODEL_MANIFEST_FILE_NAME, STAGING_DIR_PREFIX};
use crate::domain::{required_platform, ModelAsset, ModelManifest};
use crate::ports::{
    ArtifactFetchError, ArtifactFetchErrorKind, ArtifactFetcherPort, InvalidAssetKind,
    ModelInstallPortError, ModelInstallPortErrorKind, ModelInstallerPort, ModelStagingArea,
    StagedInstallOutcome,
};
use crate::LocalModelAssetStore;

/// 测试用 engine_revision（canonical segment：字母数字开头、无 `..`）。
const TEST_ENGINE_REVISION: &str = "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b";

/// 生产布局的三类小体积夹具资产（model.gguf / pointer_head / tokenizer）。
fn fixture_payloads() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("model.gguf", b"GGUF-TEST-WEIGHTS-CONTENT".to_vec()),
        ("pointer_head.safetensors", vec![7_u8; 64]),
        ("tokenizer/merges.txt", b"line-one\nline-two\n".to_vec()),
    ]
}

/// 按夹具字节构造契约 manifest（长度与 SHA-256 均来自真实字节，单一来源）。
fn fixture_manifest() -> ModelManifest {
    let assets = fixture_payloads()
        .into_iter()
        .map(|(path, payload)| ModelAsset {
            url: format!("https://example.com/{path}"),
            byte_length: payload.len() as u64,
            sha256: utils::sha256_hex(&payload),
            path: path.to_owned(),
        })
        .collect();
    ModelManifest {
        schema_version: 1,
        engine_revision: TEST_ENGINE_REVISION.to_owned(),
        hidden_size: 1024,
        pointer_dimension: 256,
        temperature: 0.07,
        supported_platforms: vec![required_platform().to_owned()],
        assets,
    }
}

/// fake 抓取端口的一次调用记录（URL、相对路径、staging 根与推导出的目标路径）。
#[derive(Debug, Clone)]
struct RecordedFetch {
    url: String,
    relative_path: String,
    staging_root: PathBuf,
    destination: PathBuf,
}

/// 脚本化抓取行为：写入指定字节，或返回 typed 抓取失败（不写任何字节）。
#[derive(Debug, Clone)]
enum FetchPlan {
    /// 把给定字节写入目标文件（fake 自行创建父目录）。
    Write(Vec<u8>),
    /// 返回 typed 抓取失败。
    Fail(ArtifactFetchErrorKind),
}

/// fake `ArtifactFetcherPort`：记录 URL/path/staging 根/调用次数，
/// 按脚本写小夹具或注入失败；未脚本化的资产写正确夹具字节。
#[derive(Default)]
struct ScriptedArtifactFetcher {
    /// 按序记录的调用。
    calls: Mutex<Vec<RecordedFetch>>,
    /// asset path → 脚本行为（缺省 = 写正确夹具字节）。
    plans: Mutex<HashMap<String, FetchPlan>>,
    /// 首次调用时执行的副作用（模拟并发竞争者在下载期间落地）。
    first_call_side_effect: Mutex<Option<Box<dyn FnOnce() + Send + 'static>>>,
}

impl ScriptedArtifactFetcher {
    /// 调用记录快照。
    fn recorded_calls(&self) -> Vec<RecordedFetch> {
        self.calls.lock().expect("calls 锁").clone()
    }

    /// 注入某资产的脚本行为。
    fn plan_for(&self, relative_path: &str, plan: FetchPlan) {
        self.plans
            .lock()
            .expect("plans 锁")
            .insert(relative_path.to_owned(), plan);
    }

    /// 注入首次调用副作用（一次性）。
    fn on_first_call(&self, side_effect: impl FnOnce() + Send + 'static) {
        *self.first_call_side_effect.lock().expect("hook 锁") = Some(Box::new(side_effect));
    }
}

#[async_trait]
impl ArtifactFetcherPort for ScriptedArtifactFetcher {
    async fn fetch_asset_into_staging(
        &self,
        staging_root: &Path,
        asset: &ModelAsset,
    ) -> Result<(), ArtifactFetchError> {
        let destination = staging_root.join(&asset.path);
        self.calls.lock().expect("calls 锁").push(RecordedFetch {
            url: asset.url.clone(),
            relative_path: asset.path.clone(),
            staging_root: staging_root.to_path_buf(),
            destination: destination.clone(),
        });
        if let Some(side_effect) = self.first_call_side_effect.lock().expect("hook 锁").take() {
            side_effect();
        }
        let plan = self
            .plans
            .lock()
            .expect("plans 锁")
            .get(&asset.path)
            .cloned();
        match plan {
            Some(FetchPlan::Fail(kind)) => Err(ArtifactFetchError {
                kind,
                detail: "脚本注入的抓取失败".to_owned(),
            }),
            plan_write => {
                let bytes = match plan_write {
                    Some(FetchPlan::Write(bytes)) => bytes,
                    _ => fixture_payloads()
                        .into_iter()
                        .find(|(path, _)| *path == asset.path)
                        .map(|(_, payload)| payload)
                        .expect("夹具资产应存在"),
                };
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).expect("夹具父目录创建");
                }
                std::fs::write(&destination, &bytes).expect("夹具写入");
                Ok(())
            }
        }
    }
}

/// fake 安装端口：委托真实 `LocalModelAssetStore`，并记录 `discard_staging`
/// 显式调用次数 —— 用于证明失败路径的清理是 **application 显式 await**，
/// 而不是 RAII `Drop` 假绿（Drop 不经过端口，计数不会增加）。
struct CountingInstaller {
    /// 委托的真实存储。
    inner: Arc<LocalModelAssetStore>,
    /// `discard_staging` 显式调用计数。
    discard_calls: Mutex<usize>,
}

impl CountingInstaller {
    /// `discard_staging` 显式调用次数。
    fn discard_calls(&self) -> usize {
        *self.discard_calls.lock().expect("discard 计数锁")
    }
}

#[async_trait]
impl ModelInstallerPort for CountingInstaller {
    async fn create_staging(&self) -> Result<ModelStagingArea, ModelInstallPortError> {
        self.inner.create_staging().await
    }

    async fn install_staged(
        &self,
        staging: ModelStagingArea,
    ) -> Result<StagedInstallOutcome, ModelInstallPortError> {
        self.inner.install_staged(staging).await
    }

    async fn discard_staging(
        &self,
        staging: ModelStagingArea,
    ) -> Result<(), ModelInstallPortError> {
        *self.discard_calls.lock().expect("discard 计数锁") += 1;
        self.inner.discard_staging(staging).await
    }
}

/// 下载用例测试骨架：临时安装根 + 注入 manifest/store/fetcher。
struct DownloadHarness {
    /// 每测试唯一临时目录。
    _temp_dir: tempfile::TempDir,
    /// 安装根目录。
    root_dir: PathBuf,
    /// 注入的固定 manifest。
    manifest: ModelManifest,
    /// 真实本地存储（同时作 `ModelAssetPort` 与计数安装端口的委托目标）。
    store: Arc<LocalModelAssetStore>,
    /// 记录显式 `discard_staging` 调用次数的安装端口。
    installer: Arc<CountingInstaller>,
    /// fake 抓取端口。
    fetcher: Arc<ScriptedArtifactFetcher>,
}

impl DownloadHarness {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("临时目录");
        let root_dir = temp_dir.path().join("models").join("systemone");
        let manifest = fixture_manifest();
        let store = Arc::new(
            LocalModelAssetStore::new(root_dir.clone(), manifest.clone())
                .expect("契约 manifest 应通过校验"),
        );
        let installer = Arc::new(CountingInstaller {
            inner: store.clone(),
            discard_calls: Mutex::new(0),
        });
        let fetcher = Arc::new(ScriptedArtifactFetcher::default());
        Self {
            _temp_dir: temp_dir,
            root_dir,
            manifest,
            store,
            installer,
            fetcher,
        }
    }

    fn service(&self) -> ModelDownloadService {
        ModelDownloadService::new(
            self.manifest.clone(),
            self.store.clone(),
            self.installer.clone(),
            self.fetcher.clone(),
        )
        .expect("合法 manifest 应通过构造校验")
    }

    /// root 下全部暂存目录（断言无半成品残留）。
    fn staging_directories(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.root_dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(STAGING_DIR_PREFIX))
            })
            .collect()
    }

    /// 期望的 final revision 目录。
    fn revision_dir(&self) -> PathBuf {
        self.root_dir.join(&self.manifest.engine_revision)
    }
}

/// 在 root 下手工构造完整有效的 revision 安装树（读路径输入，不经过安装入口）。
fn write_valid_revision_tree(root_dir: &Path, manifest: &ModelManifest) -> PathBuf {
    let revision_dir = root_dir.join(&manifest.engine_revision);
    std::fs::create_dir_all(&revision_dir).expect("revision 目录创建");
    for (path, payload) in fixture_payloads() {
        let file_path = revision_dir.join(path);
        std::fs::create_dir_all(file_path.parent().expect("资产应有父目录")).expect("父目录创建");
        std::fs::write(file_path, payload).expect("资产写入");
    }
    let serialized = serde_json::to_string_pretty(manifest).expect("manifest 序列化");
    std::fs::write(revision_dir.join(MODEL_MANIFEST_FILE_NAME), serialized).expect("manifest 写入");
    revision_dir
}

#[tokio::test]
async fn valid_cache_returns_already_installed_without_network_calls() {
    let harness = DownloadHarness::new();
    let revision_dir = write_valid_revision_tree(&harness.root_dir, &harness.manifest);
    let service = harness.service();

    let outcome = service.download().await.expect("有效缓存应幂等成功");

    assert!(matches!(outcome, DownloadOutcome::AlreadyInstalled { .. }));
    assert_eq!(outcome.revision(), harness.manifest.engine_revision);
    assert_eq!(outcome.install_root(), revision_dir.as_path());
    assert_eq!(outcome.exit_code(), 0, "幂等成功 MUST 映射退出码 0");
    assert!(
        harness.fetcher.recorded_calls().is_empty(),
        "有效缓存 MUST 零网络调用"
    );
    assert!(
        harness.staging_directories().is_empty(),
        "有效缓存 MUST 不创建暂存目录"
    );
}

#[tokio::test]
async fn invalid_cache_fails_without_network_or_staging_and_keeps_content() {
    let harness = DownloadHarness::new();
    let revision_dir = write_valid_revision_tree(&harness.root_dir, &harness.manifest);
    let corrupt_path = revision_dir.join("model.gguf");
    std::fs::write(&corrupt_path, b"CORRUPTED-BYTES").expect("损坏内容写入");
    let service = harness.service();

    let error = service
        .download()
        .await
        .expect_err("Invalid 缓存 MUST 失败");

    assert!(
        matches!(
            error.kind,
            ModelDownloadErrorKind::InvalidInstallState(InvalidAssetKind::Corrupt)
        ),
        "应为 typed Invalid 状态错误：{:?}",
        error.kind
    );
    assert_eq!(error.exit_code(), 1, "失败 MUST 映射退出码 1");
    assert!(
        harness.fetcher.recorded_calls().is_empty(),
        "Invalid 缓存 MUST 零网络调用"
    );
    assert!(
        harness.staging_directories().is_empty(),
        "Invalid 缓存 MUST 不创建暂存目录"
    );
    assert_eq!(
        std::fs::read(&corrupt_path).expect("损坏内容可读"),
        b"CORRUPTED-BYTES",
        "Invalid 安装 MUST NOT 被覆盖"
    );
}

#[tokio::test]
async fn missing_assets_download_all_in_manifest_order_and_install() {
    let harness = DownloadHarness::new();
    let service = harness.service();

    let outcome = service.download().await.expect("缺失应下载并安装成功");

    assert!(matches!(outcome, DownloadOutcome::Installed { .. }));
    assert_eq!(outcome.revision(), harness.manifest.engine_revision);
    assert_eq!(outcome.install_root(), harness.revision_dir().as_path());
    assert_eq!(outcome.exit_code(), 0, "安装成功 MUST 映射退出码 0");
    for (path, payload) in fixture_payloads() {
        let written = std::fs::read(harness.revision_dir().join(path)).expect("已安装资产可读");
        assert_eq!(written, payload, "资产 {path} 内容应与夹具一致");
    }
    assert!(
        harness
            .revision_dir()
            .join(MODEL_MANIFEST_FILE_NAME)
            .is_file(),
        "final MUST 落 canonical manifest"
    );
    assert!(harness.staging_directories().is_empty(), "staging 应已清理");
    assert_eq!(
        harness.installer.discard_calls(),
        0,
        "安装成功路径由 install_staged 消费暂存，MUST 零显式 discard_staging"
    );

    let calls = harness.fetcher.recorded_calls();
    let expected_paths: Vec<&str> = harness
        .manifest
        .assets
        .iter()
        .map(|asset| asset.path.as_str())
        .collect();
    let recorded_paths: Vec<&str> = calls
        .iter()
        .map(|call| call.relative_path.as_str())
        .collect();
    assert_eq!(
        recorded_paths, expected_paths,
        "MUST 按 manifest 顺序逐资产下载"
    );
    let expected_urls: Vec<&str> = harness
        .manifest
        .assets
        .iter()
        .map(|asset| asset.url.as_str())
        .collect();
    let recorded_urls: Vec<&str> = calls.iter().map(|call| call.url.as_str()).collect();
    assert_eq!(
        recorded_urls, expected_urls,
        "MUST 使用 manifest 声明的 URL"
    );
}

#[tokio::test]
async fn fetch_network_failure_discards_staging_and_leaves_no_final() {
    let harness = DownloadHarness::new();
    harness.fetcher.plan_for(
        "pointer_head.safetensors",
        FetchPlan::Fail(ArtifactFetchErrorKind::Network),
    );
    let service = harness.service();

    let error = service.download().await.expect_err("抓取失败 MUST 失败");

    assert!(
        matches!(
            error.kind,
            ModelDownloadErrorKind::Fetch(ArtifactFetchErrorKind::Network)
        ),
        "应为 typed 抓取失败：{:?}",
        error.kind
    );
    assert_eq!(error.exit_code(), 1, "抓取失败 MUST 映射退出码 1");
    assert_eq!(
        harness.installer.discard_calls(),
        1,
        "抓取失败 MUST 恰好一次显式 discard_staging（不能靠 RAII Drop 兜底假绿）"
    );
    assert!(
        harness.staging_directories().is_empty(),
        "失败 MUST 显式清理 staging"
    );
    assert!(!harness.revision_dir().exists(), "失败 MUST 不出现 final");
}

#[tokio::test]
async fn length_mismatch_reported_by_fetcher_fails_closed() {
    let harness = DownloadHarness::new();
    harness.fetcher.plan_for(
        "model.gguf",
        FetchPlan::Fail(ArtifactFetchErrorKind::LengthMismatch),
    );
    let service = harness.service();

    let error = service.download().await.expect_err("长度不符 MUST 失败");

    assert!(matches!(
        error.kind,
        ModelDownloadErrorKind::Fetch(ArtifactFetchErrorKind::LengthMismatch)
    ));
    assert_eq!(
        harness.installer.discard_calls(),
        1,
        "抓取长度失败 MUST 恰好一次显式 discard_staging"
    );
    assert!(
        harness.staging_directories().is_empty(),
        "staging MUST 清理"
    );
    assert!(!harness.revision_dir().exists(), "final MUST 不出现");
}

#[tokio::test]
async fn length_mismatch_detected_by_installer_verification_fails_closed() {
    let harness = DownloadHarness::new();
    harness
        .fetcher
        .plan_for("model.gguf", FetchPlan::Write(b"too-short".to_vec()));
    let service = harness.service();

    let error = service
        .download()
        .await
        .expect_err("安装端最终校验 MUST 拦截长度不符");

    assert!(
        matches!(
            error.kind,
            ModelDownloadErrorKind::Install(ModelInstallPortErrorKind::VerificationFailed)
        ),
        "应为 typed 安装校验失败：{:?}",
        error.kind
    );
    assert!(
        error.detail.contains("字节数"),
        "detail 应说明长度不符：{}",
        error.detail
    );
    assert_eq!(error.exit_code(), 1);
    assert!(
        harness.staging_directories().is_empty(),
        "staging MUST 清理"
    );
    assert!(!harness.revision_dir().exists(), "final MUST 不出现");
}

#[tokio::test]
async fn sha_mismatch_detected_by_installer_final_verification_fails_closed() {
    let harness = DownloadHarness::new();
    let expected_length = harness.manifest.assets[0].byte_length;
    harness.fetcher.plan_for(
        "model.gguf",
        FetchPlan::Write(vec![0xA5_u8; expected_length as usize]),
    );
    let service = harness.service();

    let error = service
        .download()
        .await
        .expect_err("安装端最终校验 MUST 拦截 SHA 不符");

    assert!(
        matches!(
            error.kind,
            ModelDownloadErrorKind::Install(ModelInstallPortErrorKind::VerificationFailed)
        ),
        "应为 typed 安装校验失败：{:?}",
        error.kind
    );
    assert!(
        error.detail.contains("SHA-256"),
        "detail 应说明 SHA 不符：{}",
        error.detail
    );
    assert!(
        harness.staging_directories().is_empty(),
        "staging MUST 清理"
    );
    assert!(!harness.revision_dir().exists(), "final MUST 不出现");
}

#[tokio::test]
async fn invalid_manifest_is_rejected_at_service_construction() {
    let harness = DownloadHarness::new();
    let mut invalid_manifest = harness.manifest.clone();
    invalid_manifest.hidden_size = 512;

    let error = ModelDownloadService::new(
        invalid_manifest,
        harness.store.clone(),
        harness.store.clone(),
        harness.fetcher.clone(),
    )
    .expect_err("结构错误 manifest MUST 在构造期被拒绝");

    assert!(
        matches!(error.kind, ModelDownloadErrorKind::ManifestRejected),
        "应为 manifest 契约拒绝：{:?}",
        error.kind
    );
    assert_eq!(error.exit_code(), 1);
    assert!(
        harness.fetcher.recorded_calls().is_empty(),
        "构造拒绝 MUST 零网络调用"
    );
    assert!(
        harness.staging_directories().is_empty(),
        "构造拒绝 MUST 无暂存目录"
    );
}

#[tokio::test]
async fn concurrent_valid_revision_is_kept_untouched_and_outcome_is_idempotent() {
    let harness = DownloadHarness::new();
    let side_effect_root = harness.root_dir.clone();
    let side_effect_manifest = harness.manifest.clone();
    harness.fetcher.on_first_call(move || {
        // 模拟并发竞争者在下载期间落地有效安装（附带哨兵文件证明未被覆盖）。
        let revision_dir = write_valid_revision_tree(&side_effect_root, &side_effect_manifest);
        std::fs::write(revision_dir.join("competitor.sentinel"), b"competitor").expect("哨兵写入");
    });
    let service = harness.service();

    let outcome = service
        .download()
        .await
        .expect("竞争者已落地有效安装应幂等成功");

    assert!(matches!(outcome, DownloadOutcome::AlreadyInstalled { .. }));
    assert_eq!(outcome.install_root(), harness.revision_dir().as_path());
    assert!(
        harness.revision_dir().join("competitor.sentinel").is_file(),
        "已有有效 revision MUST NOT 被覆盖"
    );
    assert!(harness.staging_directories().is_empty(), "staging 应已清理");
}

#[tokio::test]
async fn download_exit_codes_map_success_to_zero_and_failure_to_non_zero() {
    // 成功（幂等命中）→ 0。
    let success_harness = DownloadHarness::new();
    write_valid_revision_tree(&success_harness.root_dir, &success_harness.manifest);
    let success_outcome = success_harness
        .service()
        .download()
        .await
        .expect("幂等成功");
    assert_eq!(success_outcome.exit_code(), 0);

    // 失败（抓取失败）→ 1。
    let failure_harness = DownloadHarness::new();
    failure_harness.fetcher.plan_for(
        "model.gguf",
        FetchPlan::Fail(ArtifactFetchErrorKind::Network),
    );
    let failure_error = failure_harness
        .service()
        .download()
        .await
        .expect_err("抓取失败 MUST 失败");
    assert_eq!(failure_error.exit_code(), 1);
}

#[tokio::test]
async fn fetch_destinations_follow_manifest_relative_layout_under_one_staging_root() {
    let harness = DownloadHarness::new();
    let service = harness.service();

    service.download().await.expect("缺失应下载并安装成功");

    let calls = harness.fetcher.recorded_calls();
    assert_eq!(
        calls.len(),
        harness.manifest.assets.len(),
        "MUST 每资产恰好一次抓取"
    );
    let staging_root = &calls[0].staging_root;
    assert_eq!(
        staging_root.parent(),
        Some(harness.root_dir.as_path()),
        "staging MUST 是安装根的直接子目录"
    );
    assert!(
        staging_root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(STAGING_DIR_PREFIX)),
        "staging MUST 带隐藏暂存前缀"
    );
    for (call, asset) in calls.iter().zip(&harness.manifest.assets) {
        assert_eq!(
            &call.staging_root, staging_root,
            "全部资产 MUST 写入同一 staging 根"
        );
        assert_eq!(
            call.relative_path, asset.path,
            "相对路径 MUST 来自 manifest"
        );
        assert_eq!(
            call.destination,
            staging_root.join(&asset.path),
            "目标路径 MUST 是 staging 根下的 manifest 相对布局"
        );
    }
}
