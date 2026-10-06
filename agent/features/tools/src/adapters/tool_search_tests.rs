use super::*;

fn make_tool(name: &str, description: &str) -> ToolInfo {
    ToolInfo {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: serde_json::json!({"type": "object"}),
        is_read_only: true,
    }
}

#[test]
fn test_compute_relevance_exact_name_match() {
    let tool = make_tool("Bash", "Execute shell commands");
    assert_eq!(compute_relevance("bash", &tool), Some(100.0));
}

#[test]
fn test_compute_relevance_name_contains() {
    let tool = make_tool("ToolSearch", "Search for tools");
    assert_eq!(compute_relevance("search", &tool), Some(80.0));
}

#[test]
fn test_compute_relevance_desc_contains() {
    let tool = make_tool("Bash", "Execute shell commands");
    assert_eq!(compute_relevance("shell", &tool), Some(50.0));
}

#[test]
fn test_compute_relevance_no_match() {
    let tool = make_tool("Bash", "Execute shell commands");
    assert_eq!(compute_relevance("file", &tool), None);
}

#[test]
fn test_compute_relevance_case_insensitive() {
    let tool = make_tool("Read", "Read file contents");
    // query 应该是小写的（调用方已转换）
    assert_eq!(compute_relevance("read", &tool), Some(100.0));
    assert_eq!(compute_relevance("read file", &tool), Some(50.0));
}

// ---------- #1835 System One 语义重排 ----------

use crate::domain::{
    ExecutionScope, FixedGuidance, MutexReadSet, ToolExecutionContext, ToolExecutionPorts,
    ToolListProvider, WorkspaceReadAccess,
};
use async_trait::async_trait;
use project::WorkspaceReader;
use share::error::DomainError;
use share::session_types::{ProjectIdentityData, WorkspaceId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use systemone::{ScoringAnswer, ScoringPort, ScoringQuestion, ScoringState, UnavailableKind};

struct TestCancellation;
#[async_trait]
impl crate::domain::CancellationSignal for TestCancellation {
    fn is_cancelled(&self) -> bool {
        false
    }
    async fn cancelled(&self) {
        std::future::pending::<()>().await
    }
    fn child_signal(&self) -> Arc<dyn crate::domain::CancellationSignal> {
        Arc::new(Self)
    }
}

struct TestWorkspace {
    root: std::path::PathBuf,
}

impl WorkspaceReader for TestWorkspace {
    fn current_workspace_root(&self) -> std::path::PathBuf {
        self.root.clone()
    }
    fn workspace_id(&self) -> WorkspaceId {
        WorkspaceId::new("tool-search-test")
    }
    fn project_identity(&self) -> ProjectIdentityData {
        ProjectIdentityData {
            initial_cwd: self.root.display().to_string(),
            git_common_dir: None,
        }
    }
    fn current_path_base(&self) -> std::path::PathBuf {
        self.root.clone()
    }
    fn resolve(&self, path: &std::path::Path) -> std::path::PathBuf {
        self.root.join(path)
    }
    fn resolve_file_path(&self, path: &std::path::Path) -> Result<std::path::PathBuf, DomainError> {
        Ok(self.resolve(path))
    }
    fn resolve_search_path(
        &self,
        path: &std::path::Path,
    ) -> Result<std::path::PathBuf, DomainError> {
        Ok(self.resolve(path))
    }
    fn in_worktree(&self) -> bool {
        false
    }
    fn current_branch(&self) -> Result<Option<String>, DomainError> {
        Ok(None)
    }
    fn initial_cwd(&self) -> std::path::PathBuf {
        self.root.clone()
    }
}

struct FakeCatalog {
    tools: Vec<ToolInfo>,
}

impl ToolListProvider for FakeCatalog {
    fn tool_names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name.clone()).collect()
    }
    fn tool_description(&self, name: &str) -> Option<String> {
        self.tools
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.description.clone())
    }
    fn tool_info(&self, name: &str) -> Option<ToolInfo> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }
}

/// 可编程评分 mock：预设每候选概率表（criteria key = 候选序号）或强制失败。
struct FakeScoring {
    probabilities: Option<Vec<(String, f64)>>,
    fail: bool,
    calls: AtomicUsize,
}

#[async_trait]
impl ScoringPort for FakeScoring {
    async fn answer(
        &self,
        _state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, systemone::ScoringUnavailable> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(questions.len(), 1, "ToolSearch 单题调用");
        if self.fail {
            return Err(systemone::ScoringUnavailable::new(
                UnavailableKind::Timeout,
                "test forced failure",
            ));
        }
        let probabilities = self.probabilities.clone().expect("测试未配置概率");
        let choice = probabilities
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(key, _)| key.clone())
            .unwrap_or_default();
        Ok(vec![ScoringAnswer::Choice {
            choice,
            probabilities,
            confidence: 0.9,
            calibration: systemone::CalibrationLevel::Raw,
        }])
    }
}

fn search_context(
    catalog: Arc<FakeCatalog>,
    scoring: Option<Arc<FakeScoring>>,
) -> ToolExecutionContext {
    let scope = ExecutionScope::builder(
        "tool-search-test-run",
        WorkspaceId::new("tool-search-test"),
        std::path::PathBuf::from("/tmp"),
    )
    .build();
    let ports = ToolExecutionPorts::new(
        Arc::new(TestCancellation),
        WorkspaceReadAccess::new(Arc::new(TestWorkspace {
            root: std::path::PathBuf::from("/tmp"),
        })),
        Arc::new(MutexReadSet(Arc::new(std::sync::Mutex::new(
            Default::default(),
        )))),
        Arc::new(memory::api::NoOpMemory),
        Arc::new(FixedGuidance {
            language: "en".into(),
        }),
    )
    .with_catalog(Some(catalog))
    .with_scoring(scoring.map(|port| port as Arc<dyn ScoringPort>));
    ToolExecutionContext::new(scope, ports)
}

fn semantic_catalog() -> Vec<ToolInfo> {
    vec![
        make_tool("Bash", "Execute shell commands in a persistent session"),
        make_tool("Archify", "Create polished architecture diagrams as HTML"),
        make_tool("WebSearch", "Search the web for current information"),
        make_tool("Makefile", "Run makefile commands in a build session"),
    ]
}

#[tokio::test]
async fn low_confidence_query_triggers_scoring_rerank() {
    // "画一张系统架构图"：词法对三个工具零命中 → 触发评分，按概率序返回。
    let catalog = Arc::new(FakeCatalog {
        tools: semantic_catalog(),
    });
    let scoring = Arc::new(FakeScoring {
        probabilities: Some(vec![
            ("0".to_string(), 0.1),  // Bash
            ("1".to_string(), 0.7),  // Archify
            ("2".to_string(), 0.2),  // WebSearch
            ("3".to_string(), 0.05), // Makefile
        ]),
        fail: false,
        calls: AtomicUsize::new(0),
    });
    let ctx = search_context(catalog, Some(scoring.clone()));
    let tool = ToolSearchTool;
    let result = tool
        .call(serde_json::json!({"query": "画一张系统架构图"}), &ctx)
        .await;
    let output = result.data.expect("应成功");
    let names: Vec<&str> = output.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Archify", "WebSearch", "Bash", "Makefile"],
        "按概率序"
    );
    assert_eq!(
        scoring.calls.load(Ordering::SeqCst),
        1,
        "零命中必须触发评分"
    );
}

#[tokio::test]
async fn confident_lexical_hit_skips_scoring() {
    // "bash"：exact 命中 → 短路，不触发评分。
    let catalog = Arc::new(FakeCatalog {
        tools: semantic_catalog(),
    });
    let scoring = Arc::new(FakeScoring {
        probabilities: Some(vec![("0".to_string(), 1.0)]),
        fail: false,
        calls: AtomicUsize::new(0),
    });
    let ctx = search_context(catalog, Some(scoring.clone()));
    let tool = ToolSearchTool;
    let result = tool.call(serde_json::json!({"query": "bash"}), &ctx).await;
    let output = result.data.expect("应成功");
    assert_eq!(output.tools.len(), 1);
    assert_eq!(output.tools[0].name, "Bash");
    assert_eq!(
        scoring.calls.load(Ordering::SeqCst),
        0,
        "高置信短路不触发评分"
    );
}

#[tokio::test]
async fn scoring_failure_falls_back_to_lexical_order() {
    // desc-only 双命中（低置信，候选 ≥2）但评分失败 → 回退词法序（desc 命中集）。
    let catalog = Arc::new(FakeCatalog {
        tools: semantic_catalog(),
    });
    let scoring = Arc::new(FakeScoring {
        probabilities: None,
        fail: true,
        calls: AtomicUsize::new(0),
    });
    let ctx = search_context(catalog, Some(scoring.clone()));
    let tool = ToolSearchTool;
    // "commands"：Bash 与 Makefile desc contains（50 分，低置信双命中）。
    let result = tool
        .call(serde_json::json!({"query": "commands"}), &ctx)
        .await;
    let output = result.data.expect("应成功");
    assert_eq!(output.tools.len(), 2, "降级回退词法命中集");
    assert_eq!(output.tools[0].name, "Bash", "同分保持原相对序");
    assert_eq!(output.tools[1].name, "Makefile");
    assert_eq!(scoring.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn no_scoring_port_keeps_pure_lexical_behavior() {
    // 未注入 port（开关关）：零命中与现状一致。
    let catalog = Arc::new(FakeCatalog {
        tools: semantic_catalog(),
    });
    let ctx = search_context(catalog, None);
    let tool = ToolSearchTool;
    let result = tool
        .call(serde_json::json!({"query": "画一张系统架构图"}), &ctx)
        .await;
    let output = result.data.expect("应成功");
    assert!(output.tools.is_empty(), "开关关零命中返回空");
}
