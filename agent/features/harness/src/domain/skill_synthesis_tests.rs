use crate::adapters::filesystem::FilesystemComponentRegistry;
use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentId, ComponentKind, ComponentName};
use crate::domain::errors::HarnessError;
use crate::domain::evidence::{EvidenceSourceRef, SignalKind};
use crate::domain::improvement::RsiLlmPort;
use crate::domain::skill_synthesis::synthesize_skill_from_trace;
use crate::domain::task_trace::{TaskAction, TaskActionKind, TaskTrace, VerificationFact};
use crate::domain::versioning::{ComponentVersionStore, SnapshotContent};
use crate::domain::virtual_registry::VirtualComponentRegistry;
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

struct ScriptedLlm {
    responses: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<String>>,
}

impl ScriptedLlm {
    fn new(responses: impl IntoIterator<Item = String>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            prompts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RsiLlmPort for ScriptedLlm {
    async fn complete(&self, prompt: &str) -> Result<String, HarnessError> {
        self.prompts
            .lock()
            .expect("prompts")
            .push(prompt.to_string());
        Ok(self
            .responses
            .lock()
            .expect("responses")
            .pop_front()
            .expect("scripted response"))
    }
}

fn id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::Skill,
        name: ComponentName::new(name),
    }
}

fn registry_under(root: &std::path::Path, names: &[&str]) -> FilesystemComponentRegistry {
    FilesystemComponentRegistry::new(
        names
            .iter()
            .map(|name| (id(name), root.join(format!("{name}.md"))))
            .collect::<HashMap<_, _>>(),
    )
}

fn create_response(name: &str, description: &str, evidence_refs: &[&str]) -> String {
    serde_json::json!({
        "decision": "create",
        "name": name,
        "description": description,
        "when_to_use": ["处理边界明确的已授权 issue"],
        "steps": ["读取 issue", "在授权范围内最小修改", "运行测试"],
        "verification": ["cargo test -p harness --lib"],
        "pitfalls": ["读取 issue 不代表可以修改 issue 或 merge"],
        "evidence_refs": evidence_refs,
        "reason": "真实交付轨迹"
    })
    .to_string()
}

fn trace(command: &str) -> TaskTrace {
    TaskTrace {
        session_id: "project/session.dataset".to_string(),
        run_id: "run-1".to_string(),
        user_goal: "修复一个缺陷并交付 PR".to_string(),
        changed_files: vec!["src/lib.rs".to_string()],
        actions: vec![
            TaskAction {
                kind: TaskActionKind::Edit,
                tool: "Edit".to_string(),
                command: Some("修改 src/lib.rs".to_string()),
                success: Some(true),
                target_path: Some("src/lib.rs".to_string()),
                excerpt: None,
            },
            TaskAction {
                kind: TaskActionKind::Test,
                tool: "Bash".to_string(),
                command: Some(command.to_string()),
                success: Some(true),
                target_path: Some("src/lib.rs".to_string()),
                excerpt: None,
            },
            TaskAction {
                kind: TaskActionKind::Pr,
                tool: "Bash".to_string(),
                command: Some("gh pr create --title fix".to_string()),
                success: Some(true),
                target_path: Some("src/lib.rs".to_string()),
                excerpt: None,
            },
        ],
        verification: vec![VerificationFact {
            command: "cargo test -p harness --lib".to_string(),
            success: Some(true),
            excerpt: None,
        }],
        outcome: "Completed".to_string(),
        sources: vec![EvidenceSourceRef {
            session_id: "project/session.dataset".to_string(),
            run_id: "run-1".to_string(),
            step_id: "step-1".to_string(),
        }],
    }
}

#[tokio::test]
async fn synthesis_writes_real_filesystem_with_safe_yaml_and_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("issue-to-pr.md");
    let mut registry = registry_under(dir.path(), &["issue-to-pr"]);
    let store = ComponentVersionStore::new(dir.path().join("versions"));
    let description = "修复: issue\n保留 \"授权\" 边界";
    let llm = ScriptedLlm::new([create_response("issue-to-pr", description, &["E0"])]);
    let mut preview = Vec::new();

    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test -p harness --lib"),
        false,
        &mut preview,
        None,
        Some(&store),
        "cycle-test",
    )
    .await
    .expect("synthesis")
    .expect("应生成 skill");

    assert_eq!(adoption.id.kind, ComponentKind::Skill);
    let written = std::fs::read_to_string(&path).expect("真实文件已写入");
    assert!(
        preview.join("\n").contains(&written),
        "preview 必须含完整正文"
    );
    let parsed = crate::adapters::filesystem::parse_markdown_with_frontmatter(&written)
        .expect("合法 YAML frontmatter");
    assert_eq!(
        parsed
            .frontmatter
            .and_then(|value| value.get("description").cloned()),
        Some(serde_json::Value::String(description.to_string()))
    );
    assert_eq!(adoption.evidence[0].signal, SignalKind::CommonAction);
    assert_eq!(
        adoption.evidence[0].source,
        trace("cargo test").sources.first().cloned()
    );
    let snapshot = store
        .load(&adoption.id, Some("cycle-test-skill-trace"))
        .expect("调用方可按既有 cycle 引用加载快照");
    assert!(matches!(snapshot.payload, SnapshotContent::Tombstone));
}

#[tokio::test]
async fn dry_run_is_virtual_readable_without_touching_filesystem() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("dry-run-skill.md");
    let mut base = registry_under(dir.path(), &["dry-run-skill"]);
    let llm = ScriptedLlm::new([create_response("dry-run-skill", " dry: run ", &["E0"])]);
    let mut preview = Vec::new();

    {
        let mut virtual_registry = VirtualComponentRegistry::new(&mut base);
        let adoption = synthesize_skill_from_trace(
            &mut virtual_registry,
            &llm,
            &trace("cargo test"),
            true,
            &mut preview,
            None,
            None,
            "cycle-dry-run",
        )
        .await
        .expect("dry-run synthesis")
        .expect("虚拟生成 skill");
        let virtual_body = virtual_registry
            .read(&adoption.id)
            .expect("Compose 可读取")
            .body;
        assert!(virtual_body.contains("# dry-run-skill"));
        assert!(preview.join("\n").contains(&virtual_body));
    }

    assert!(!path.exists(), "dry-run 不得写底层文件");
}

#[tokio::test]
async fn invalid_empty_unknown_and_duplicate_evidence_refs_are_rejected() {
    for refs in [vec![], vec!["E9"], vec!["E0", "E0"]] {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("evidence-skill.md");
        let mut registry = registry_under(dir.path(), &["evidence-skill"]);
        let store = ComponentVersionStore::new(dir.path().join("versions"));
        let llm = ScriptedLlm::new([create_response("evidence-skill", "证据绑定", &refs)]);
        let mut preview = Vec::new();
        let adoption = synthesize_skill_from_trace(
            &mut registry,
            &llm,
            &trace("cargo test"),
            false,
            &mut preview,
            None,
            Some(&store),
            "cycle-evidence",
        )
        .await
        .expect("非法引用应作为拒绝而非基础设施错误");
        assert!(adoption.is_none(), "refs 应被拒绝：{refs:?}");
        assert!(!path.exists());
        assert!(preview.join("\n").contains("evidence_ref"));
    }
}

#[tokio::test]
async fn malformed_json_retries_once_and_then_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("retry-skill.md");
    let mut registry = registry_under(dir.path(), &["retry-skill"]);
    let store = ComponentVersionStore::new(dir.path().join("versions"));
    let llm = ScriptedLlm::new([
        "not json".to_string(),
        create_response("retry-skill", "重试后生成", &["E0"]),
    ]);

    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        Some(&store),
        "cycle-retry",
    )
    .await
    .expect("retry synthesis")
    .expect("第二次响应可用");
    assert_eq!(adoption.id, id("retry-skill"));
    assert_eq!(llm.prompts.lock().expect("prompts").len(), 2);
    assert!(path.exists());
}

#[tokio::test]
async fn formal_write_without_snapshot_store_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("fail-closed.md");
    let mut registry = registry_under(dir.path(), &["fail-closed"]);
    let llm = ScriptedLlm::new([create_response("fail-closed", "禁止无快照写入", &["E0"])]);

    let error = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        None,
        "cycle-no-store",
    )
    .await
    .expect_err("正式写入缺 snapshot store 必须失败");
    assert!(error.message().contains("snapshot store"));
    assert!(!path.exists());
}

#[tokio::test]
async fn full_raw_and_specialized_similar_skill_reach_llm_without_hard_rejection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let existing_path = dir.path().join("issue-fix.md");
    let specialized_path = dir.path().join("issue-fix-specialized.md");
    let response = create_response("issue-fix-specialized", "仅处理特化场景", &["E0"]);
    let tail_marker = "第三百字符之后仍必须参与去重裁决的特化约束";
    let existing_body = format!(
        "---\ndescription: \"仅处理通用场景\"\n---\n# issue-fix\n\n{}\n\n{tail_marker}\n",
        "相同步骤内容。\n".repeat(40)
    );
    std::fs::write(&existing_path, &existing_body).expect("existing raw");
    let mut registry = registry_under(dir.path(), &["issue-fix", "issue-fix-specialized"]);
    let store = ComponentVersionStore::new(dir.path().join("versions"));
    let llm = ScriptedLlm::new([
        r#"{"decision":"create_new","reason":"现有 skill 不能承接"}"#.to_string(),
        response,
        r#"{"verdict":"specialization","reason":"保留项目/场景差异"}"#.to_string(),
    ]);

    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        Some(&store),
        "cycle-dedup",
    )
    .await
    .expect("synthesis")
    .expect("相似仅召回，特化 skill 由 LLM 决定创建");
    assert_eq!(adoption.id, id("issue-fix-specialized"));
    assert!(specialized_path.exists());
    assert_eq!(
        std::fs::read_to_string(existing_path).expect("existing"),
        existing_body
    );
    let prompt = llm.prompts.lock().expect("prompts")[2].clone();
    assert!(
        prompt.contains(tail_marker),
        "去重仲裁必须读取完整 raw，而非前 300 字符"
    );
    assert!(prompt.contains("召回"));
    assert!(
        llm.prompts.lock().expect("prompts")[1].contains("读取 issue 不代表可以修改 issue、merge")
    );
    assert!(llm.prompts.lock().expect("prompts")[1].contains("tool_success=Some(true)"));
    assert!(llm.prompts.lock().expect("prompts")[1].contains("不等于任务成功"));
}

#[tokio::test]
async fn no_change_preserves_different_existing_skill() {
    let dir = tempfile::tempdir().expect("tempdir");
    let existing_path = dir.path().join("existing.md");
    let existing_raw = "# 完全不同的既有技能\n\n不得覆盖。\n";
    std::fs::write(&existing_path, existing_raw).expect("existing");
    let mut registry = registry_under(dir.path(), &["existing", "different"]);
    let llm = ScriptedLlm::new([
        serde_json::json!({"decision":"no_change","reason":"目标与适用范围不同但无需新增"})
            .to_string(),
    ]);

    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        Some(&ComponentVersionStore::new(dir.path().join("versions"))),
        "cycle-no-change",
    )
    .await
    .expect("no_change");
    assert!(adoption.is_none());
    assert_eq!(
        std::fs::read_to_string(existing_path).expect("existing"),
        existing_raw
    );
    assert!(!dir.path().join("different.md").exists());
}

#[tokio::test]
async fn synthesis_rejects_secret_trace_without_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("secret.md");
    let mut registry = registry_under(dir.path(), &["secret"]);
    let llm = ScriptedLlm::new([]);
    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("sk-test-secret cargo test"),
        false,
        &mut Vec::new(),
        None,
        None,
        "cycle-test",
    )
    .await
    .expect("synthesis");
    assert!(adoption.is_none());
    assert!(!path.exists());
}

#[test]
fn candidate_rejects_empty_goal_or_missing_edit() {
    let mut candidate = trace("cargo test");
    candidate.user_goal = String::new();
    assert!(candidate.candidate_score() < 0);
    candidate = trace("cargo test");
    candidate
        .actions
        .retain(|action| action.kind != TaskActionKind::Edit);
    assert!(candidate.candidate_score() < 0);
}

#[tokio::test]
async fn duplicate_skill_is_rejected_without_touching_existing_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let existing_path = dir.path().join("issue-to-pr.md");
    let existing_raw = "---\ndescription: \"处理 issue\"\n---\n# issue-to-pr\n\n读取 issue，最小修改，运行测试。\n";
    std::fs::write(&existing_path, existing_raw).unwrap();
    let mut registry = registry_under(dir.path(), &["issue-to-pr", "issue-to-pr-copy"]);
    let llm = ScriptedLlm::new([
        r#"{"decision":"create_new","reason":"先尝试新建再查重"}"#.to_string(),
        create_response("issue-to-pr-copy", "处理 issue 并验证", &["E0"]),
        r#"{"verdict":"duplicate","reason":"目标、步骤和约束等价"}"#.to_string(),
    ]);
    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        Some(&ComponentVersionStore::new(dir.path().join("versions"))),
        "cycle-duplicate",
    )
    .await
    .expect("dedup");
    assert!(adoption.is_none());
    assert_eq!(
        std::fs::read_to_string(existing_path).unwrap(),
        existing_raw
    );
    assert!(!dir.path().join("issue-to-pr-copy.md").exists());
}

#[tokio::test]
async fn actual_skill_directory_persists_yaml_frontmatter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut registry = crate::adapters::skills_registry::SkillsDirectoryRegistry::new(dir.path());
    let store = ComponentVersionStore::new(dir.path().join("versions"));
    let llm = ScriptedLlm::new([create_response(
        "issue-to-pr",
        "处理 issue: 保留 \"授权\"",
        &["E0"],
    )]);
    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        Some(&store),
        "cycle-real-skill",
    )
    .await
    .expect("write actual skill")
    .expect("created");
    let path = dir.path().join("issue-to-pr/SKILL.md");
    let raw = std::fs::read_to_string(&path).expect("real SKILL.md");
    assert!(raw.contains("description:"));
    let content = registry.read(&adoption.id).expect("skill registry reread");
    assert_eq!(
        content.frontmatter.as_ref().unwrap()["description"],
        "处理 issue: 保留 \"授权\""
    );
    let snapshot = store
        .load(&adoption.id, Some("cycle-real-skill-skill-trace"))
        .unwrap();
    assert!(matches!(snapshot.payload, SnapshotContent::Tombstone));
}

#[tokio::test]
async fn reuse_existing_skill_does_not_create_or_modify_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("issue-to-pr.md");
    let raw = "---\ndescription: \"处理 issue\"\n---\n# issue-to-pr\n\n读取 issue。\n";
    std::fs::write(&path, raw).unwrap();
    let mut registry = registry_under(dir.path(), &["issue-to-pr"]);
    let llm = ScriptedLlm::new([
        r#"{"decision":"reuse_existing","skill":"issue-to-pr","reason":"已有 skill 已覆盖"}"#
            .to_string(),
    ]);
    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        None,
        "cycle-reuse",
    )
    .await
    .expect("reuse");
    assert!(adoption.is_none());
    assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
}

#[tokio::test]
async fn revise_existing_skill_uses_patch_and_preserves_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("issue-to-pr.md");
    let raw = "---\ndescription: \"处理 issue\"\n---\n# issue-to-pr\n\n## 步骤\n- 读取 issue\n";
    std::fs::write(&path, raw).unwrap();
    let mut registry = registry_under(dir.path(), &["issue-to-pr"]);
    let store = ComponentVersionStore::new(dir.path().join("versions"));
    let llm = ScriptedLlm::new([
        r#"{"decision":"revise_existing","skill":"issue-to-pr","reason":"补验证"}"#.to_string(),
        r#"{"decision":"revise","evidence_refs":["E0"],"reason":"补验证","operations":[{"op":"replace_exact","old_string":"- 读取 issue\n","new_string":"- 读取 issue\n- 运行测试\n"}]}"#.to_string(),
    ]);
    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        Some(&store),
        "cycle-revise",
    )
    .await
    .expect("revise")
    .expect("adoption");
    assert_eq!(adoption.id, id("issue-to-pr"));
    let updated = std::fs::read_to_string(path).unwrap();
    assert!(updated.contains("- 运行测试"));
    let snapshot = store
        .load(&adoption.id, Some("cycle-revise-skill-revision"))
        .unwrap();
    assert!(matches!(snapshot.payload, SnapshotContent::Existing { raw: old } if old == raw));
}

#[tokio::test]
async fn reuse_does_not_require_create_grade_trace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("build-cli.md");
    let raw = "---\ndescription: \"使用 build_cli 构建\"\n---\n# build-cli\n";
    std::fs::write(&path, raw).unwrap();
    let mut registry = registry_under(dir.path(), &["build-cli"]);
    let mut task = trace("cargo test");
    task.actions.clear();
    task.verification.clear();
    task.user_goal = "更新，执行build_cli.sh".into();
    assert!(task.candidate_score() < 8);
    let llm = ScriptedLlm::new([
        r#"{"decision":"reuse_existing","skill":"build-cli","reason":"直接使用现有构建技能"}"#
            .to_string(),
    ]);
    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &task,
        false,
        &mut Vec::new(),
        None,
        None,
        "cycle-reuse-only",
    )
    .await
    .expect("reuse");
    assert!(adoption.is_none());
    assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
}

#[test]
fn candidate_selection_keeps_reuse_only_trace_for_disposition() {
    let mut task = trace("cargo test");
    task.actions.clear();
    task.verification.clear();
    task.user_goal = "使用 build_cli.sh".into();
    assert!(task.candidate_score() < 8);
    let traces = vec![task];
    let selected = crate::domain::skill_synthesis::select_skill_candidate(&traces)
        .expect("reuse-only trace 应进入承接判定");
    assert_eq!(selected.user_goal, "使用 build_cli.sh");
}

#[tokio::test]
async fn revision_rejects_whole_document_rewrite() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("issue-to-pr.md");
    let raw = "---\ndescription: \"处理 issue\"\n---\n# issue-to-pr\n\n读取 issue。\n";
    std::fs::write(&path, raw).unwrap();
    let mut registry = registry_under(dir.path(), &["issue-to-pr"]);
    let llm = ScriptedLlm::new([
        r#"{"decision":"revise_existing","skill":"issue-to-pr","reason":"重写"}"#.to_string(),
        serde_json::json!({
            "decision":"revise", "evidence_refs":["E0"], "reason":"整篇替换",
            "operations":[{"op":"replace_exact","old_string":raw,"new_string":"---\ndescription: \"new\"\n---\n# new\n"}]
        }).to_string(),
    ]);
    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &trace("cargo test"),
        false,
        &mut Vec::new(),
        None,
        Some(&ComponentVersionStore::new(dir.path().join("versions"))),
        "cycle-whole-rewrite",
    )
    .await
    .expect("reject");
    assert!(adoption.is_none());
    assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
}

#[tokio::test]
async fn disposition_redacts_secret_commands_before_llm() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("build-cli.md");
    std::fs::write(&path, "---\ndescription: \"构建\"\n---\n# build-cli\n").unwrap();
    let mut registry = registry_under(dir.path(), &["build-cli"]);
    let mut task = trace("cargo test");
    task.actions[0].command = Some("sk-very-secret-value cargo test".into());
    let llm =
        ScriptedLlm::new([r#"{"decision":"no_action","reason":"证据含敏感信息"}"#.to_string()]);
    let adoption = synthesize_skill_from_trace(
        &mut registry,
        &llm,
        &task,
        false,
        &mut Vec::new(),
        None,
        None,
        "cycle-redact",
    )
    .await
    .expect("no action");
    assert!(adoption.is_none());
    let prompt = llm.prompts.lock().unwrap()[0].clone();
    assert!(!prompt.contains("sk-very-secret-value"));
    assert!(prompt.contains("sk-***"));
}
