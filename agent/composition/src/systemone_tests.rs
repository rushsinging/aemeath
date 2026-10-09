//! System One 装配矩阵契约：全关零成本、typed 启动结果、场景槽位分配与
//! 生产源 HTTP 退役边界（设计 §4.1 / §4.3）。

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// 记录被咨询次数的发行 manifest 源（零成本路径必须 NEVER 咨询它）。
struct RecordingSource {
    consulted: AtomicUsize,
    manifest: Option<systemone::ModelManifest>,
}

impl RecordingSource {
    fn with_manifest(manifest: systemone::ModelManifest) -> Self {
        Self {
            consulted: AtomicUsize::new(0),
            manifest: Some(manifest),
        }
    }

    fn empty() -> Self {
        Self {
            consulted: AtomicUsize::new(0),
            manifest: None,
        }
    }
}

impl ReleaseManifestSource for RecordingSource {
    fn release_manifest(&self) -> Option<systemone::ModelManifest> {
        self.consulted.fetch_add(1, Ordering::SeqCst);
        self.manifest.clone()
    }
}

/// 记录装配次数与注入 manifest 的 fake embedded 工厂（不触碰模型目录 / worker）。
struct RecordingFactory {
    available: bool,
    wired: AtomicUsize,
    last_revision: Mutex<Option<String>>,
    result: Result<Arc<dyn systemone::ScoringPort>, systemone::EmbeddedScoringWiringError>,
}

impl RecordingFactory {
    fn returning(
        available: bool,
        result: Result<Arc<dyn systemone::ScoringPort>, systemone::EmbeddedScoringWiringError>,
    ) -> Self {
        Self {
            available,
            wired: AtomicUsize::new(0),
            last_revision: Mutex::new(None),
            result,
        }
    }
}

#[async_trait::async_trait]
impl EmbeddedScoringFactory for RecordingFactory {
    fn available(&self) -> bool {
        self.available
    }

    async fn wire(
        &self,
        manifest: &systemone::ModelManifest,
    ) -> Result<Arc<dyn systemone::ScoringPort>, systemone::EmbeddedScoringWiringError> {
        self.wired.fetch_add(1, Ordering::SeqCst);
        *self.last_revision.lock().expect("revision 锁") = Some(manifest.engine_revision.clone());
        self.result.clone()
    }
}

/// fake 评分端口：只证明槽位被分配，不进行任何推理。
struct StubScoringPort;

#[async_trait::async_trait]
impl systemone::ScoringPort for StubScoringPort {
    async fn answer(
        &self,
        _state: &systemone::ScoringState,
        _questions: &[systemone::ScoringQuestion],
    ) -> Result<Vec<systemone::ScoringAnswer>, systemone::ScoringUnavailable> {
        Ok(Vec::new())
    }
}

/// 测试 fixture manifest（URL / SHA-256 仅锁契约格式，**不**对应任何真实发行数据）。
fn fixture_manifest() -> systemone::ModelManifest {
    systemone::ModelManifest {
        schema_version: 1,
        engine_revision: "composition-fixture-r1".to_owned(),
        hidden_size: 1024,
        pointer_dimension: 256,
        temperature: 0.07,
        supported_platforms: vec!["macos-aarch64".to_owned()],
        assets: vec![
            systemone::ModelAsset {
                path: "model.gguf".to_owned(),
                url: "https://models.example.com/systemone/model.gguf".to_owned(),
                byte_length: 775_000_000,
                sha256: "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0"
                    .to_owned(),
            },
            systemone::ModelAsset {
                path: "pointer_head.safetensors".to_owned(),
                url: "https://models.example.com/systemone/pointer_head.safetensors".to_owned(),
                byte_length: 8_403_456,
                sha256: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90"
                    .to_owned(),
            },
            systemone::ModelAsset {
                path: "tokenizer/tokenizer.json".to_owned(),
                url: "https://models.example.com/systemone/tokenizer/tokenizer.json".to_owned(),
                byte_length: 263_456,
                sha256: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
                    .to_owned(),
            },
        ],
    }
}

fn all_enabled() -> share::config::ScoringConfig {
    share::config::ScoringConfig {
        enabled: true,
        memory_rerank: true,
        memory_recall: true,
        skill_match: true,
        policy_triage: true,
    }
}

fn assert_no_ports(assignment: &ScoringPortAssignment) {
    assert!(
        assignment.for_memory_rerank.is_none(),
        "失败/关闭路径必须回退原路径（槽位 None）"
    );
    assert!(assignment.for_memory_recall.is_none());
    assert!(assignment.for_skill_match.is_none());
    assert!(assignment.for_policy_triage.is_none());
}

/// 场景开关全关：零成本——不咨询 manifest 源、不启动装配、无端口。
#[tokio::test]
async fn all_switches_disabled_is_zero_cost() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(true, Ok(Arc::new(StubScoringPort)));
    let assembly =
        assemble_scoring_ports_with(&share::config::ScoringConfig::default(), &factory, &source)
            .await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Disabled);
    assert_no_ports(&assembly.assignment);
    assert_eq!(
        source.consulted.load(Ordering::SeqCst),
        0,
        "全关时不得解析发行 manifest"
    );
    assert_eq!(
        factory.wired.load(Ordering::SeqCst),
        0,
        "全关时不得启动 embedded 装配"
    );
}

/// 开关开启 + 构建无 embedded 能力：typed `EmbeddedUnavailable`，
/// 不读 manifest、不装配、两个槽位 None、绝不 HTTP 回退。
#[tokio::test]
async fn embedded_unavailable_returns_typed_outcome_without_manifest_read() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(false, Ok(Arc::new(StubScoringPort)));
    let assembly = assemble_scoring_ports_with(&all_enabled(), &factory, &source).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::EmbeddedUnavailable);
    assert_no_ports(&assembly.assignment);
    assert_eq!(source.consulted.load(Ordering::SeqCst), 0);
    assert_eq!(factory.wired.load(Ordering::SeqCst), 0);
}

/// 开关开启 + 发行 manifest 未提供：typed `ManifestUnavailable`（NEVER 假数据）。
#[tokio::test]
async fn missing_release_manifest_returns_manifest_unavailable() {
    let source = RecordingSource::empty();
    let factory = RecordingFactory::returning(true, Ok(Arc::new(StubScoringPort)));
    let assembly = assemble_scoring_ports_with(&all_enabled(), &factory, &source).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::ManifestUnavailable);
    assert_no_ports(&assembly.assignment);
    assert_eq!(factory.wired.load(Ordering::SeqCst), 0);
}

/// 开关开启 + 模型未安装：typed `ModelMissing`（detail 提示手动下载命令）。
#[tokio::test]
async fn model_missing_maps_to_typed_outcome() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(
        true,
        Err(systemone::EmbeddedScoringWiringError::ModelMissing),
    );
    let assembly = assemble_scoring_ports_with(&all_enabled(), &factory, &source).await;
    assert!(
        matches!(assembly.outcome, ScoringStartupOutcome::ModelMissing { .. }),
        "错误类型不符：{:?}",
        assembly.outcome
    );
    let ScoringStartupOutcome::ModelMissing { detail } = &assembly.outcome else {
        panic!("outcome 分支已断言");
    };
    assert!(
        detail.contains("aemeath systemone download"),
        "detail 应提示下载命令：{detail}"
    );
    assert_no_ports(&assembly.assignment);
    assert_eq!(factory.wired.load(Ordering::SeqCst), 1);
    assert_eq!(
        factory.last_revision.lock().expect("锁").as_deref(),
        Some("composition-fixture-r1"),
        "注入的发行 manifest 必须透传给工厂"
    );
}

/// 开关开启 + 资产损坏：typed `InvalidAssets` 携带类别与中文 detail。
#[tokio::test]
async fn invalid_assets_outcome_carries_kind_and_detail() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(
        true,
        Err(systemone::EmbeddedScoringWiringError::InvalidAssets {
            kind: systemone::InvalidAssetKind::Corrupt,
            detail: "model.gguf 的 sha256 与 manifest 不符".to_owned(),
        }),
    );
    let assembly = assemble_scoring_ports_with(&all_enabled(), &factory, &source).await;
    assert!(
        matches!(
            assembly.outcome,
            ScoringStartupOutcome::InvalidAssets { .. }
        ),
        "错误类型不符：{:?}",
        assembly.outcome
    );
    let ScoringStartupOutcome::InvalidAssets { detail } = &assembly.outcome else {
        panic!("outcome 分支已断言");
    };
    assert!(
        detail.contains("已损坏") && detail.contains("sha256 与 manifest 不符"),
        "detail 应包含类别与原因：{detail}"
    );
    assert_no_ports(&assembly.assignment);
}

/// 开关开启 + 初始化失败（tokenizer / PointerHead / worker / manifest 契约）：
/// typed `InitFailed` 携带中文 detail，主聊天不被阻断（槽位 None 即回退）。
#[tokio::test]
async fn init_failure_outcome_carries_detail() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(
        true,
        Err(systemone::EmbeddedScoringWiringError::TokenizerLoadFailed {
            detail: "tokenizer.json 解析失败".to_owned(),
        }),
    );
    let assembly = assemble_scoring_ports_with(&all_enabled(), &factory, &source).await;
    assert!(
        matches!(assembly.outcome, ScoringStartupOutcome::InitFailed { .. }),
        "错误类型不符：{:?}",
        assembly.outcome
    );
    let ScoringStartupOutcome::InitFailed { detail } = &assembly.outcome else {
        panic!("outcome 分支已断言");
    };
    assert!(
        detail.contains("tokenizer.json 解析失败"),
        "detail 应携带失败原因：{detail}"
    );
    assert_no_ports(&assembly.assignment);
}

/// 装配成功：`Ready` + 按场景开关分配槽位（关闭的场景仍为 None）。
#[tokio::test]
async fn ready_assigns_ports_per_enabled_switch() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(true, Ok(Arc::new(StubScoringPort)));

    let rerank_only = share::config::ScoringConfig {
        memory_rerank: true,
        ..share::config::ScoringConfig::default()
    };
    let assembly = assemble_scoring_ports_with(&rerank_only, &factory, &source).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Ready);
    assert!(
        assembly.assignment.for_memory_rerank.is_some(),
        "rerank 开关开启且装配成功时槽位非 None"
    );
    assert!(
        assembly.assignment.for_memory_recall.is_none(),
        "recall 开关关闭时槽位必须为 None"
    );

    let both = assemble_scoring_ports_with(&all_enabled(), &factory, &source).await;
    assert_eq!(both.outcome, ScoringStartupOutcome::Ready);
    assert!(both.assignment.for_memory_rerank.is_some());
    assert!(both.assignment.for_memory_recall.is_some());
}

/// 生产装配入口（默认构建，无 embedded feature）：typed `EmbeddedUnavailable`。
#[cfg(not(feature = "systemone-embedded"))]
#[tokio::test]
async fn production_assembly_with_default_features_reports_embedded_unavailable() {
    let assembly = assemble_scoring_ports(&all_enabled()).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::EmbeddedUnavailable);
    assert_no_ports(&assembly.assignment);
}

/// 生产装配入口（embedded feature 开启 + 发行 manifest 已落地 + CI 无已安装
/// 模型）：typed `ModelMissing`（生产 manifest 常量含真实 URL / SHA， NEVER
/// 占位假数据；真实安装后的 Ready 链路由 wiring fake 覆盖）。
#[cfg(feature = "systemone-embedded")]
#[tokio::test]
async fn production_assembly_with_release_manifest_reports_model_missing_when_uninstalled() {
    // 生产 manifest 的 revision 目录不得存在于本测试运行环境（CI 恒无模型；
    // 本机若已安装同 revision 则直接命中 Ready——断言二选一，两者都是合法终态）。
    let assembly = assemble_scoring_ports(&all_enabled()).await;
    match assembly.outcome {
        ScoringStartupOutcome::ModelMissing { .. } => assert_no_ports(&assembly.assignment),
        ScoringStartupOutcome::Ready => assert!(assembly.assignment.for_memory_rerank.is_some()),
        other => panic!("生产装配结果应为 ModelMissing 或 Ready，实际 {other:?}"),
    }
}

/// 生产 composition 源文件永远不得引用 HTTP 评分工厂或其配置 env。
#[test]
fn production_scoring_sources_never_reference_http_scoring_factory() {
    let source_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in ["runtime.rs", "systemone.rs"] {
        let source = std::fs::read_to_string(source_dir.join(file))
            .unwrap_or_else(|error| panic!("读取 composition {file}：{error}"));
        for forbidden in [
            "wire_http_scoring_port",
            "wire_scoring_port",
            "JevHttpScoringAdapter",
            "AEMEATH_SCORING_URL",
            "AEMEATH_SCORING_MODEL",
            "AEMEATH_SCORING_TIMEOUT_MS",
        ] {
            assert!(
                !source.contains(forbidden),
                "production composition {file} 不得引用 HTTP scoring 符号 `{forbidden}`"
            );
        }
    }
}

/// 启动提醒转换：`Disabled` / `Ready` 无提醒（零成本与正常态不打扰用户）。
#[test]
fn scoring_startup_notices_empty_for_disabled_and_ready() {
    assert!(scoring_startup_notices(&ScoringStartupOutcome::Disabled).is_empty());
    assert!(scoring_startup_notices(&ScoringStartupOutcome::Ready).is_empty());
}

/// 启动提醒转换：`ModelMissing` 携带一条含手动下载命令的中文提醒。
#[test]
fn scoring_startup_notices_model_missing_mentions_download_command() {
    let notices = scoring_startup_notices(&ScoringStartupOutcome::ModelMissing {
        detail: "System One 模型未安装，评分功能不可用；执行 `aemeath systemone download` 安装"
            .to_string(),
    });
    assert_eq!(notices.len(), 1);
    assert!(notices[0].message.contains("aemeath systemone download"));
}

/// 启动提醒转换：其余 typed outcome 各生成一条非空中文提醒
/// （场景开关开启但评分未生效时用户必须可感知）。
#[test]
fn scoring_startup_notices_non_empty_for_unavailable_outcomes() {
    for outcome in [
        ScoringStartupOutcome::EmbeddedUnavailable,
        ScoringStartupOutcome::ManifestUnavailable,
        ScoringStartupOutcome::InvalidAssets {
            detail: "资产损坏".to_string(),
        },
        ScoringStartupOutcome::InitFailed {
            detail: "初始化失败".to_string(),
        },
    ] {
        let notices = scoring_startup_notices(&outcome);
        assert_eq!(
            notices.len(),
            1,
            "outcome {outcome:?} 应生成恰好一条启动提醒"
        );
        assert!(!notices[0].message.is_empty());
    }
}

/// 生产发行 manifest 常量契约：三资产 URL 均指向 HF 官方域（https）、SHA-256
/// 为 64 位十六进制、维度与 kev 0.8B 决策口径一致、平台仅 macOS arm64。
#[test]
fn production_release_manifest_matches_hf_release_shape() {
    let manifest = ProductionReleaseManifest
        .release_manifest()
        .expect("生产 manifest 必须落地");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.engine_revision, "kev-0.8b-q8-r1");
    assert_eq!(manifest.hidden_size, 1024);
    assert_eq!(manifest.pointer_dimension, 256);
    assert!((manifest.temperature - 2.351_095_8).abs() < 1e-6);
    assert_eq!(
        manifest.supported_platforms,
        vec!["macos-aarch64".to_string()]
    );
    assert_eq!(manifest.assets.len(), 3);
    let expected_paths = [
        "model.gguf",
        "pointer_head.safetensors",
        "tokenizer/tokenizer.json",
    ];
    for (asset, expected_path) in manifest.assets.iter().zip(expected_paths) {
        assert_eq!(asset.path, expected_path);
        assert!(
            asset.url.starts_with(
                "https://huggingface.co/rushsinging/aemeath-systemone-kev/resolve/main/"
            ),
            "URL 必须指向 HF 发行仓库：{}",
            asset.url
        );
        assert_eq!(asset.sha256.len(), 64);
        assert!(asset.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(asset.byte_length > 0);
    }
    // Q8_0 GGUF 资产量级 sanity（约 812MB；明显偏离说明常量填错）。
    let gguf = manifest
        .assets
        .iter()
        .find(|asset| asset.path == "model.gguf")
        .expect("model.gguf 资产");
    assert!(
        gguf.byte_length > 700_000_000 && gguf.byte_length < 900_000_000,
        "Q8_0 GGUF 大小异常：{}",
        gguf.byte_length
    );
}

/// HF xet/LFS CDN 重定向 host 白名单非空且全部为 HF 官方域（下载链 302 必需）。
#[test]
fn hf_cdn_redirect_hosts_are_official_hf_domains() {
    assert!(!crate::constants::HF_CDN_REDIRECT_HOSTS.is_empty());
    for host in crate::constants::HF_CDN_REDIRECT_HOSTS {
        assert!(
            host.ends_with(".hf.co") || host.ends_with(".huggingface.co"),
            "CDN host 必须是 HF 官方域：{host}"
        );
    }
}

/// 槽位与开关一一对应：skill_match 单开（memory 全关）时 `for_skill_match`
/// 有端口而 `for_memory_recall` 为 None——修复 skill_match 依赖 recall 槽导致
/// 的单开静默失效。
#[tokio::test]
async fn skill_match_alone_owns_its_slot_without_memory_slots() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(true, Ok(Arc::new(StubScoringPort)));
    let skill_only = share::config::ScoringConfig {
        skill_match: true,
        ..share::config::ScoringConfig::default()
    };
    let assembly = assemble_scoring_ports_with(&skill_only, &factory, &source).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Ready);
    assert!(
        assembly.assignment.for_skill_match.is_some(),
        "skill_match 开关开启且装配成功时必须产出自己的槽位"
    );
    assert!(assembly.assignment.for_memory_rerank.is_none());
    assert!(assembly.assignment.for_memory_recall.is_none());
}

/// 槽位与开关一一对应：policy_triage 单开时产出自己的槽位（消费端接入随
/// 场景任务交付，槽位先就位保证「开开关加载模型却无人消费」不再发生）。
#[tokio::test]
async fn policy_triage_alone_owns_its_slot() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(true, Ok(Arc::new(StubScoringPort)));
    let triage_only = share::config::ScoringConfig {
        policy_triage: true,
        ..share::config::ScoringConfig::default()
    };
    let assembly = assemble_scoring_ports_with(&triage_only, &factory, &source).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Ready);
    assert!(assembly.assignment.for_policy_triage.is_some());
    assert!(assembly.assignment.for_skill_match.is_none());
}

/// 总闸门（#1832）：`enabled=false` 时无视场景开关走零成本路径
///（Disabled、不咨询 manifest、不装配）。
#[tokio::test]
async fn master_gate_disabled_forces_zero_cost_even_with_scenarios_on() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let factory = RecordingFactory::returning(true, Ok(Arc::new(StubScoringPort)));
    let gated_off = share::config::ScoringConfig {
        enabled: false,
        memory_rerank: true,
        skill_match: true,
        ..share::config::ScoringConfig::default()
    };
    let assembly = assemble_scoring_ports_with(&gated_off, &factory, &source).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Disabled);
    assert_no_ports(&assembly.assignment);
    assert_eq!(
        source.consulted.load(Ordering::SeqCst),
        0,
        "总闸门关闭时不得解析发行 manifest"
    );
    assert_eq!(factory.wired.load(Ordering::SeqCst), 0);
}

/// 总闸门默认开（`enabled=true` 缺省）：场景开关语义与现状完全一致。
#[tokio::test]
async fn master_gate_defaults_to_enabled() {
    assert!(share::config::ScoringConfig::default().enabled);
    let rerank_on = share::config::ScoringConfig {
        memory_rerank: true,
        ..share::config::ScoringConfig::default()
    };
    assert!(
        rerank_on.enabled,
        "缺省 enabled 必须为 true（配置文件兼容）"
    );
}
