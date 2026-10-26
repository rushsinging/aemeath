//! 系统提示组装契约（#1863 W4）。
//!
//! 覆盖：块顺序、静态提示占位替换、allow_all 改写、agent 名册注入条件、
//! guidance 缓存与通知端口触发、后台进程段门控。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use share::config::domain::snapshot::ConfigSnapshot;
use share::config::AgentInstanceConfig;
use share::config::Config;
use share::config::PermissionModeConfig;
use share::reasoning::ReasoningLevel;

use super::{SessionPromptInputsData, SystemPromptAssembly};
use crate::domain::{ContextRequestData, Language};

/// 记录通知调用的假实现（Hook BC 端口）。
#[derive(Default)]
struct RecordingNotifier {
    calls: Mutex<Vec<(String, String)>>,
}

impl share::instructions::InstructionsLoadedNotifier for RecordingNotifier {
    fn instructions_loaded(&self, file_path: &str, instruction_type: &str) {
        self.calls
            .lock()
            .unwrap()
            .push((file_path.to_string(), instruction_type.to_string()));
    }
}

fn snapshot_with(mutate: impl FnOnce(&mut Config)) -> ConfigSnapshot {
    let mut config = Config::default();
    mutate(&mut config);
    ConfigSnapshot::new(config)
}

fn request_with(
    snapshot: ConfigSnapshot,
    workspace_is_git: bool,
    model_id: &str,
    language: Language,
) -> ContextRequestData {
    ContextRequestData {
        session_id: crate::domain::SessionId::new("session"),
        request_id: share::ids::InteractionRequestId::new("irq-test"),
        run_id: share::ids::RunId::new("run"),
        step_id: share::ids::RunStepId::new("step"),
        pending_messages: Vec::new(),
        workspace_root: std::path::PathBuf::from("/tmp/pl-assembly-project"),
        workspace_is_git,
        model_id: model_id.to_string(),
        effective_reasoning: ReasoningLevel::Off,
        language,
        agent_roles: HashMap::new(),
        config_snapshot: snapshot,
        context_size: 200_000,
        max_output_tokens: 8192,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: Vec::new(),
        tool_schema_tokens: 0,
    }
}

/// 静态提示占位符按 request 事实替换：cwd 与 git 标记都来自请求。
#[tokio::test]
async fn static_block_substitutes_workspace_facts() {
    let assembly = SystemPromptAssembly::new(None);
    let request = request_with(snapshot_with(|_| {}), true, "test/model", Language::En);
    let blocks = assembly.materialize(&request).await;

    let static_block = blocks
        .iter()
        .find(|block| block.kind == "system_prompt")
        .expect("静态提示块必须存在");
    assert!(
        static_block.content.contains("/tmp/pl-assembly-project"),
        "cwd 占位必须被请求事实替换: {}",
        static_block.content
    );
    assert!(
        !static_block.content.contains("{cwd_str}"),
        "占位符不得残留"
    );
    assert!(static_block.cacheable, "静态提示属 cacheable prefix");
}

/// allow_all 模式放宽 workspace 边界表述；Ask 模式保留原文。
#[tokio::test]
async fn allow_all_mode_rewrites_workspace_boundary() {
    let ask = SystemPromptAssembly::new(None)
        .materialize(&request_with(
            snapshot_with(|config| {
                config.permissions.mode = PermissionModeConfig::Ask;
            }),
            false,
            "test/model",
            Language::En,
        ))
        .await;
    let allow_all = SystemPromptAssembly::new(None)
        .materialize(&request_with(
            snapshot_with(|config| {
                config.permissions.mode = PermissionModeConfig::AllowAll;
            }),
            false,
            "test/model",
            Language::En,
        ))
        .await;

    let ask_static = &ask[0].content;
    let allow_all_static = &allow_all[0].content;
    assert_ne!(ask_static, allow_all_static, "两种模式文案必须不同");
    assert!(
        allow_all_static.contains("informational"),
        "allow_all 必须说明 workspace_root 仅提供上下文"
    );
}

/// agent 名册段只在存在 enabled 具名实例时出现。
#[tokio::test]
async fn agent_roles_block_appears_only_with_named_instances() {
    let without_instances = SystemPromptAssembly::new(None)
        .materialize(&request_with(
            snapshot_with(|_| {}),
            false,
            "test/model",
            Language::En,
        ))
        .await;
    assert!(
        !without_instances
            .iter()
            .any(|block| block.kind == "agent_roles"),
        "无具名实例时不得产生名册段"
    );

    let mut names = HashMap::new();
    names.insert(
        "coder".to_string(),
        AgentInstanceConfig {
            role: "coder".to_string(),
            description: "写代码".to_string(),
            enabled: true,
            ..Default::default()
        },
    );
    let with_instances = SystemPromptAssembly::new(None)
        .materialize(&request_with(
            snapshot_with(|config| config.agents.names = names),
            false,
            "test/model",
            Language::En,
        ))
        .await;
    let roles = with_instances
        .iter()
        .find(|block| block.kind == "agent_roles")
        .expect("有具名实例时必须产生名册段");
    assert!(roles.content.contains("coder"));
    assert!(roles.content.contains("写代码"));
}

/// 后台进程段仅在阈值启用时注入。
#[tokio::test]
async fn background_block_follows_threshold() {
    let disabled = SystemPromptAssembly::new(None)
        .materialize(&request_with(
            snapshot_with(|config| {
                config.runtime.tool_background_threshold_secs = 0;
            }),
            false,
            "test/model",
            Language::En,
        ))
        .await;
    assert!(!disabled
        .iter()
        .any(|block| block.kind == "background_processes"));

    let enabled = SystemPromptAssembly::new(None)
        .materialize(&request_with(
            snapshot_with(|config| {
                config.runtime.tool_background_threshold_secs = 60;
            }),
            false,
            "test/model",
            Language::En,
        ))
        .await;
    let background = enabled
        .iter()
        .find(|block| block.kind == "background_processes")
        .expect("阈值启用时必须注入后台进程段");
    assert!(background.content.contains("Background processes"));
}

/// 会话级组装：块间以空行拼接，且 execution discipline 位于末尾。
#[tokio::test]
async fn session_prompt_includes_discipline_last() {
    let assembly = SystemPromptAssembly::new(None);
    let inputs = SessionPromptInputsData {
        workspace_root: std::path::PathBuf::from("/tmp/pl-session-project"),
        workspace_is_git: true,
        model_id: "test/model".to_string(),
        reasoning: false,
        language: "en".to_string(),
        config_snapshot: snapshot_with(|_| {}),
    };
    let prompt = assembly.materialize_session_prompt(&inputs).await;

    assert!(prompt.contains("/tmp/pl-session-project"));
    let discipline = share::i18n::prompt::discipline::universal_execution_discipline("en");
    assert!(
        prompt.ends_with(discipline),
        "execution discipline 必须位于末尾"
    );
    assert!(!prompt.contains("\n\n\n"), "块间不应出现连续三空行");
}

/// guidance 通知端口：每个实际加载文件上报一次，且 instruction_type 恒为 guidance。
/// 同一输入的第二次调用命中缓存，不再重复上报（磁盘 IO 与通知都被跳过）。
#[tokio::test]
async fn guidance_notification_is_per_loaded_file_and_cached() {
    let fixture = GuidanceFixture::new("assembly_notify");
    let default_file = fixture.write("_default.md", "default guidance body");
    let model_file = fixture.write("test.md", "model guidance body");
    let notifier = Arc::new(RecordingNotifier::default());
    let assembly = SystemPromptAssembly::new(Some(notifier.clone()));
    let request = request_with(snapshot_with(|_| {}), false, "test/model", Language::En);

    let first = assembly.materialize(&request).await;
    let calls = notifier.calls.lock().unwrap().clone();
    let paths: Vec<&str> = calls.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        vec![default_file.to_str().unwrap(), model_file.to_str().unwrap()],
        "每个加载文件各上报一次，顺序与拼装顺序一致"
    );
    assert!(calls.iter().all(|(_, kind)| kind == "guidance"));
    let guidance_block = first
        .iter()
        .find(|block| block.kind == "guidance")
        .expect("命中 guidance 文件时必须产出该块");
    assert!(guidance_block.content.contains("default guidance body"));
    assert!(guidance_block.content.contains("model guidance body"));

    // 同一输入再组装一次：缓存命中，不重复上报。
    let _ = assembly.materialize(&request).await;
    assert_eq!(
        notifier.calls.lock().unwrap().len(),
        calls.len(),
        "缓存命中不得重复上报"
    );
}

/// 空 guidance 目录：正文回退内建默认（无磁盘路径），因此不得产生任何上报。
///
/// 这是「通知只针对真实磁盘文件」的边界：内建默认内容不是外部加载的指令文件。
#[tokio::test]
async fn guidance_without_disk_files_reports_nothing() {
    let _fixture = GuidanceFixture::new("assembly_empty");
    let notifier = Arc::new(RecordingNotifier::default());
    let assembly = SystemPromptAssembly::new(Some(notifier.clone()));
    let request = request_with(snapshot_with(|_| {}), false, "absent/model", Language::En);

    let blocks = assembly.materialize(&request).await;
    assert!(
        notifier.calls.lock().unwrap().is_empty(),
        "无磁盘文件加载时不得上报"
    );
    // 内建默认正文仍可存在，但其来源不是外部文件。
    if let Some(guidance) = blocks.iter().find(|block| block.kind == "guidance") {
        assert!(!guidance.content.is_empty());
    }
}

/// 隔离 guidance 目录：设置 `AEMEATH_AGENTS_DIR` 后写入指定 guidance 文件。
struct GuidanceFixture {
    _lock: std::sync::MutexGuard<'static, ()>,
    root: std::path::PathBuf,
    previous: Option<std::ffi::OsString>,
}

impl GuidanceFixture {
    fn new(tag: &str) -> Self {
        // 与 guidance resolver 测试共用同一把全局 env 锁：两者都改
        // AEMEATH_AGENTS_DIR，独立锁会导致交叉污染。
        let lock = crate::adapters::prompt::GUIDANCE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let previous = std::env::var_os("AEMEATH_AGENTS_DIR");
        let root = std::env::temp_dir().join(format!("aemeath-system-prompt-{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("guidance")).unwrap();
        std::env::set_var("AEMEATH_AGENTS_DIR", &root);
        Self {
            _lock: lock,
            root,
            previous,
        }
    }

    fn write(&self, name: &str, content: &str) -> std::path::PathBuf {
        let path = self.root.join("guidance").join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for GuidanceFixture {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("AEMEATH_AGENTS_DIR", value),
            None => std::env::remove_var("AEMEATH_AGENTS_DIR"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
