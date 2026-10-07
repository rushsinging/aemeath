//! 合同测试（默认构建，不启用 `http-adapter`）：Jev HTTP 评分 adapter 与其
//! wire 类型只允许在测试 / eval 场景存在（设计 §4.3 HTTP adapter 退役边界）：
//!
//! - 公共导出 `JevHttpScoringAdapter` 与 HTTP 装配工厂 `wire_http_scoring_port`
//!   **仅**在 feature `http-adapter` 开启时存在；
//! - `jev_http` / `jev_wire` 模块只在 `cfg(any(test, feature = "http-adapter"))` 下编译；
//! - `kev_baseline` 集成测试声明 `required-features = ["http-adapter"]`；
//! - `reqwest` 必须保持非可选依赖——下载 `HttpArtifactFetcher` 仍是生产能力，
//!   评分 HTTP 与下载 HTTP 语义分离。

use std::path::PathBuf;

fn manifest_source() -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("读取 systemone Cargo.toml")
}

fn lib_source() -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .expect("读取 systemone src/lib.rs")
}

fn adapters_source() -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/adapters.rs"))
        .expect("读取 systemone src/adapters.rs")
}

/// 断言 `declaration` 所在行的上一非空行恰为 `expected_cfg` 属性。
fn assert_cfg_directly_precedes(source: &str, declaration: &str, expected_cfg: &str) {
    let lines: Vec<&str> = source.lines().collect();
    let index = lines
        .iter()
        .position(|line| line.contains(declaration))
        .unwrap_or_else(|| panic!("未找到声明 `{declaration}`"));
    let previous = lines[..index]
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_else(|| panic!("`{declaration}` 之前没有属性行"));
    assert_eq!(
        previous.trim(),
        expected_cfg,
        "`{declaration}` 必须由 `{expected_cfg}` 直接门控"
    );
}

#[test]
fn http_scoring_factory_and_public_adapter_require_http_adapter_feature() {
    let lib_source = lib_source();
    assert_cfg_directly_precedes(
        &lib_source,
        "pub fn wire_http_scoring_port(",
        "#[cfg(feature = \"http-adapter\")]",
    );
    assert_cfg_directly_precedes(
        &lib_source,
        "pub use adapters::jev_http::JevHttpScoringAdapter;",
        "#[cfg(feature = \"http-adapter\")]",
    );
    assert!(
        !lib_source.contains("fn wire_scoring_port("),
        "旧 HTTP 装配工厂 `wire_scoring_port` 已退役，不得以无门控形态回归"
    );
}

#[test]
fn jev_http_modules_compile_only_for_tests_or_http_adapter_feature() {
    let adapters_source = adapters_source();
    for module in ["pub mod jev_http;", "pub mod jev_wire;"] {
        assert_cfg_directly_precedes(
            &adapters_source,
            module,
            "#[cfg(any(test, feature = \"http-adapter\"))]",
        );
    }
}

#[test]
fn kev_baseline_integration_test_requires_http_adapter_feature() {
    let manifest_source = manifest_source();
    let blocks: Vec<&str> = manifest_source.split("[[test]]").collect();
    let baseline_block = blocks
        .iter()
        .skip(1)
        .find(|block| block.contains("kev_baseline"))
        .expect("kev_baseline 应有显式 [[test]] 声明");
    assert!(
        baseline_block.contains("required-features = [\"http-adapter\"]"),
        "kev_baseline 必须声明 required-features = [\"http-adapter\"]，默认构建不得编译它：{baseline_block}"
    );
    assert!(
        manifest_source.contains("[features]") && manifest_source.contains("http-adapter"),
        "systemone 必须定义 http-adapter feature"
    );
}

#[test]
fn reqwest_stays_required_for_production_artifact_downloader() {
    let manifest_source = manifest_source();
    assert!(
        manifest_source.contains("reqwest = { workspace = true }"),
        "下载 HttpArtifactFetcher 是生产能力，reqwest 不得整体可选"
    );
}
