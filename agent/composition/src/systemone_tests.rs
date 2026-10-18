//! System One 装配矩阵契约：全关零成本、typed 启动结果、场景槽位分配与
//! 生产源 HTTP 退役边界（设计 §4.1 / §4.3）。

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

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
}

impl ReleaseManifestSource for RecordingSource {
    fn release_manifest(&self) -> Option<systemone::ModelManifest> {
        self.consulted.fetch_add(1, Ordering::SeqCst);
        self.manifest.clone()
    }
}

/// 记录 `ensure_ready` 调用次数与结果的 fake daemon launcher。
struct RecordingDaemonLauncher {
    calls: AtomicUsize,
    result: Result<(), String>,
}

impl ScoringDaemonLauncher for RecordingDaemonLauncher {
    fn ensure_ready(
        &self,
        _sock: &std::path::Path,
        _connect_timeout: std::time::Duration,
    ) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.clone()
    }
}

#[test]
fn process_daemon_launcher_skips_lazy_spawn_under_cfg_test() {
    let sock = std::path::Path::new("/tmp/aemeath-scoring-daemon-launcher-test.sock");
    let err = ProcessDaemonLauncher
        .ensure_ready(sock, std::time::Duration::from_millis(10))
        .expect_err("cfg(test) 必须禁用惰性拉起");
    assert!(err.contains("单测禁用惰性拉起"), "unexpected detail: {err}");
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
        memory_rerank: true,
        memory_recall: true,
        skill_match: true,
        policy_triage: true,
        ..share::config::ScoringConfig::default()
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
/// 本测试不调用真实 `assemble_scoring_ports`（会触达 ProcessDaemonLauncher），
/// 改测 `assemble_daemon_scoring_ports_with` 契约。
#[tokio::test]
async fn daemon_assembly_wires_ports_and_reports_unreachable_when_launcher_fails() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let launcher = RecordingDaemonLauncher {
        calls: AtomicUsize::new(0),
        result: Err("sock 未就绪".to_string()),
    };
    let assembly = assemble_daemon_scoring_ports_with(&all_enabled(), &source, &launcher).await;
    assert_eq!(launcher.calls.load(Ordering::SeqCst), 1);
    assert!(
        matches!(
            assembly.outcome,
            ScoringStartupOutcome::DaemonUnreachable { .. }
        ),
        "outcome={:?}",
        assembly.outcome
    );
    assert!(assembly.assignment.for_memory_rerank.is_some());
    assert!(assembly.assignment.for_memory_recall.is_some());
    assert!(assembly.assignment.for_skill_match.is_some());
    assert!(assembly.assignment.for_policy_triage.is_some());
    let notices = scoring_startup_notices(&assembly.outcome);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].message.contains("sock 未就绪"));
}

#[tokio::test]
async fn daemon_assembly_ready_when_launcher_succeeds() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let launcher = RecordingDaemonLauncher {
        calls: AtomicUsize::new(0),
        result: Ok(()),
    };
    let assembly = assemble_daemon_scoring_ports_with(&all_enabled(), &source, &launcher).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Ready);
    assert!(assembly.assignment.for_memory_rerank.is_some());
    assert!(scoring_startup_notices(&assembly.outcome).is_empty());
}

#[tokio::test]
async fn daemon_assembly_disabled_is_zero_cost_without_launcher() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let launcher = RecordingDaemonLauncher {
        calls: AtomicUsize::new(0),
        result: Ok(()),
    };
    let assembly = assemble_daemon_scoring_ports_with(
        &share::config::ScoringConfig::default(),
        &source,
        &launcher,
    )
    .await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Disabled);
    assert_eq!(launcher.calls.load(Ordering::SeqCst), 0);
    assert_eq!(source.consulted.load(Ordering::SeqCst), 0);
    assert_no_ports(&assembly.assignment);
}

/// 生产 ReleaseManifest 在 daemon 装配路径仍提供 engine_revision（审计用）；
/// 不加载 GGUF。用 stub launcher 验证 Ready + 槽位。
#[tokio::test]
async fn production_release_manifest_supports_daemon_assembly_ready() {
    let launcher = RecordingDaemonLauncher {
        calls: AtomicUsize::new(0),
        result: Ok(()),
    };
    let assembly =
        assemble_daemon_scoring_ports_with(&all_enabled(), &ProductionReleaseManifest, &launcher)
            .await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Ready);
    assert!(assembly.assignment.for_memory_rerank.is_some());
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
    let systemone =
        std::fs::read_to_string(source_dir.join("systemone.rs")).expect("读取 systemone.rs");
    assert!(
        systemone.contains("assemble_daemon_scoring_ports_with"),
        "会话生产装配必须走 daemon 客户端路径"
    );
    assert!(
        systemone.contains("DaemonScoringClient"),
        "会话生产装配必须构造 DaemonScoringClient"
    );
    // `pub async fn assemble_scoring_ports` 本体不得再委托 ProductionEmbeddedFactory。
    let start = systemone
        .find("pub async fn assemble_scoring_ports(")
        .expect("assemble_scoring_ports 应存在");
    let body = &systemone[start..start + 350];
    assert!(
        body.contains("assemble_daemon_scoring_ports_with"),
        "assemble_scoring_ports 必须委托 daemon 装配：{body}"
    );
    assert!(
        !body.contains("ProductionEmbeddedFactory"),
        "assemble_scoring_ports 不得委托 ProductionEmbeddedFactory：{body}"
    );
}

/// 启动提醒转换：`Disabled` / `Ready` 无提醒（零成本与正常态不打扰用户）。
#[test]
fn scoring_startup_notices_empty_for_disabled_and_ready() {
    assert!(scoring_startup_notices(&ScoringStartupOutcome::Disabled).is_empty());
    assert!(scoring_startup_notices(&ScoringStartupOutcome::Ready).is_empty());
}

/// 启动提醒转换：其余 typed outcome 各生成一条非空中文提醒
/// （场景开关开启但评分未生效时用户必须可感知）。
#[test]
fn scoring_startup_notices_non_empty_for_unavailable_outcomes() {
    for outcome in [
        ScoringStartupOutcome::DaemonUnreachable {
            detail: "daemon 不可达".to_string(),
        },
        ScoringStartupOutcome::ManifestUnavailable,
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
    // f64 提升比较：保留发行温度全精度字面量，避免 f32 截断失真。
    assert!((f64::from(manifest.temperature) - 2.351_095_8).abs() < 1e-6);
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
    let launcher = RecordingDaemonLauncher {
        calls: AtomicUsize::new(0),
        result: Ok(()),
    };
    let skill_only = share::config::ScoringConfig {
        skill_match: true,
        ..share::config::ScoringConfig::default()
    };
    let assembly = assemble_daemon_scoring_ports_with(&skill_only, &source, &launcher).await;
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
    let launcher = RecordingDaemonLauncher {
        calls: AtomicUsize::new(0),
        result: Ok(()),
    };
    let triage_only = share::config::ScoringConfig {
        policy_triage: true,
        ..share::config::ScoringConfig::default()
    };
    let assembly = assemble_daemon_scoring_ports_with(&triage_only, &source, &launcher).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Ready);
    assert!(assembly.assignment.for_policy_triage.is_some());
    assert!(assembly.assignment.for_skill_match.is_none());
}

/// 总闸门（#1832）：`enabled=false` 时无视场景开关走零成本路径
///（Disabled、不咨询 manifest、不装配）。
#[tokio::test]
async fn master_gate_disabled_forces_zero_cost_even_with_scenarios_on() {
    let source = RecordingSource::with_manifest(fixture_manifest());
    let launcher = RecordingDaemonLauncher {
        calls: AtomicUsize::new(0),
        result: Ok(()),
    };
    let gated_off = share::config::ScoringConfig {
        enabled: false,
        memory_rerank: true,
        skill_match: true,
        ..share::config::ScoringConfig::default()
    };
    let assembly = assemble_daemon_scoring_ports_with(&gated_off, &source, &launcher).await;
    assert_eq!(assembly.outcome, ScoringStartupOutcome::Disabled);
    assert_no_ports(&assembly.assignment);
    assert_eq!(
        source.consulted.load(Ordering::SeqCst),
        0,
        "总闸门关闭时不得解析发行 manifest"
    );
    assert_eq!(launcher.calls.load(Ordering::SeqCst), 0);
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

/// serve 入口在无引擎构建下必须返回 typed Failure，且 **NEVER** 调用 ready 回调
/// （库层禁止 stdout：`#![deny(clippy::print_stdout)]`，用户输出由 CLI 渲染）。
#[cfg(not(feature = "systemone-daemon"))]
#[tokio::test]
async fn run_scoring_serve_without_engine_skips_ready_callback() {
    let called = AtomicUsize::new(0);
    let exit = run_scoring_serve(|_sock| {
        called.fetch_add(1, Ordering::SeqCst);
    })
    .await;

    assert_eq!(
        called.load(Ordering::SeqCst),
        0,
        "引擎不可用时 NEVER 调用 ready 回调"
    );
    match exit {
        ScoringServeExit::Failure { exit_code, message } => {
            assert_eq!(exit_code, 2, "feature 未启用应为 exit 2：{message}");
            assert!(
                message.contains("systemone serve"),
                "失败原因应指向 systemone serve：{message}"
            );
        }
        other => panic!("unexpected exit: {other:?}"),
    }
}
