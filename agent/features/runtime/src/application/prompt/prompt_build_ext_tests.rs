use super::*;
use share::config::AgentInstanceConfig;
use share::config::Config;
use share::i18n::prompt::discipline::universal_execution_discipline;
use std::collections::HashMap;

/// 构造一个 ConfigSnapshot，其中 `agents.names` 与 `language` 按参数设置。
/// 其余字段使用 `Config::default()`，不触碰文件系统。
fn make_snapshot(names: HashMap<String, AgentInstanceConfig>, language: &str) -> ConfigSnapshot {
    let mut config = Config::default();
    config.agents.names = names;
    config.language = language.to_string();
    share::config::domain::snapshot::ConfigSnapshot::new(config)
}

#[tokio::test]
async fn build_static_prompt_does_not_embed_execution_discipline() {
    let hook_port: Arc<dyn HookDispatcher> = hook::wire_hook_dispatcher(
        &share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config::default()),
    )
    .unwrap();
    let prompt = build_static_prompt(
        std::path::Path::new("/tmp/project"),
        "fake/model",
        false,
        None,
        &hook_port,
        crate::application::prompt::build::SystemPromptParts {
            static_part: "core-system".to_string(),
            initial_git_context: String::new(),
            claude_md: String::new(),
        },
    )
    .await;

    assert!(prompt.contains("core-system"));
    assert!(!prompt.contains(universal_execution_discipline("en")));
}

// ── append_agent_roles ────────────────────────────────────

/// ConfigSnapshot 含 2 个具名实例（coder-fast + reviewer-glm，带 description 与 model），
/// 调 append_agent_roles 后 prompt 应包含实例名 / description / model。
#[test]
fn test_append_agent_roles_with_snapshot() {
    // Arrange
    let mut names = HashMap::new();
    names.insert(
        "coder-fast".to_string(),
        AgentInstanceConfig {
            role: "coder".to_string(),
            model: "deepseek/deepseek-chat".to_string(),
            description: "Writes and edits code".to_string(),
            ..Default::default()
        },
    );
    names.insert(
        "reviewer-glm".to_string(),
        AgentInstanceConfig {
            role: "reviewer".to_string(),
            model: "anthropic/claude-sonnet-4".to_string(),
            description: "Reviews code for quality".to_string(),
            ..Default::default()
        },
    );
    let snap = make_snapshot(names, "en");
    let mut prompt = String::new();

    // Act
    append_agent_roles(&mut prompt, Some(&snap), "en");

    // Assert — 实例名、description、model 都应出现在 prompt 中
    assert!(
        prompt.contains("`coder-fast` [coder]"),
        "应包含实例名与职能（coder-fast [coder]）"
    );
    assert!(
        prompt.contains("`reviewer-glm`"),
        "应包含实例名 reviewer-glm"
    );
    assert!(
        prompt.contains("Writes and edits code"),
        "应包含 coder-fast 的 description"
    );
    assert!(
        prompt.contains("Reviews code for quality"),
        "应包含 reviewer-glm 的 description"
    );
    assert!(
        prompt.contains("deepseek/deepseek-chat"),
        "应包含 coder-fast 的 model"
    );
    assert!(
        prompt.contains("anthropic/claude-sonnet-4"),
        "应包含 reviewer-glm 的 model"
    );
}

/// 实例描述为空时回退引用职能的描述（内置 reviewer 的描述填充）。
#[test]
fn test_append_agent_roles_falls_back_to_role_description() {
    let mut names = HashMap::new();
    names.insert(
        "reviewer-ds".to_string(),
        AgentInstanceConfig {
            role: "reviewer".to_string(),
            model: "x/y".to_string(),
            ..Default::default()
        },
    );
    let snap = make_snapshot(names, "en");
    let mut prompt = String::new();

    append_agent_roles(&mut prompt, Some(&snap), "en");

    assert!(
        prompt.contains("Read-only review"),
        "内置 reviewer 职能描述应作为实例描述 fallback"
    );
}

/// 空 names 时无任何可派发实例，prompt 不追加任何内容——内置职能
/// 不隐式注入（派发必须命中具名实例）。
#[test]
fn test_append_agent_roles_empty_names_appends_nothing() {
    let snap = make_snapshot(HashMap::new(), "en");
    let mut prompt = String::from("base");

    append_agent_roles(&mut prompt, Some(&snap), "en");

    assert_eq!(prompt, "base", "空 names 时 prompt 不应追加任何 role 段");
}

/// config_file 为 None 时，append_agent_roles 应直接返回，不追加任何内容。
#[test]
fn test_append_agent_roles_none_snapshot() {
    // Arrange
    let mut prompt = String::from("base");

    // Act
    append_agent_roles(&mut prompt, None, "en");

    // Assert
    assert_eq!(
        prompt, "base",
        "config_file 为 None 时 prompt 不应追加任何内容"
    );
}

/// disabled 实例即使保留定义，也不得把它注入主 LLM。
#[test]
fn test_append_agent_roles_omits_disabled_role() {
    let mut names = HashMap::new();
    names.insert(
        "coder-fast".to_string(),
        AgentInstanceConfig {
            role: "coder".to_string(),
            enabled: false,
            description: "编写代码".to_string(),
            ..Default::default()
        },
    );
    names.insert(
        "reviewer-glm".to_string(),
        AgentInstanceConfig {
            role: "reviewer".to_string(),
            description: "审查代码".to_string(),
            ..Default::default()
        },
    );
    let snap = make_snapshot(names, "zh");
    let mut prompt = String::from("base");

    append_agent_roles(&mut prompt, Some(&snap), "zh");

    assert!(!prompt.contains("`coder-fast`"));
    assert!(prompt.contains("`reviewer-glm`"));
}

/// R3（issue #1736）：roster 必须标注绑定模型在 config.models 中的可用性，
/// 否则 LLM 无法预判哪些 agent 可用，只能靠派发报错盲试。
#[test]
fn test_append_agent_roles_marks_unavailable_bound_model() {
    let mut names = HashMap::new();
    names.insert(
        "explorer-mimo".to_string(),
        AgentInstanceConfig {
            role: "explorer".to_string(),
            model: "Mimo/mimo-v2.6-flash".to_string(),
            description: "Retrieves code".to_string(),
            ..Default::default()
        },
    );
    names.insert(
        "coder-known".to_string(),
        AgentInstanceConfig {
            role: "coder".to_string(),
            model: "test-provider/test-model".to_string(),
            description: "Writes code".to_string(),
            ..Default::default()
        },
    );
    let mut config = Config::default();
    config.agents.names = names;
    config.language = "en".to_string();
    config.models.providers.insert(
        "test-provider".to_string(),
        share::config::models::ProviderModelsConfig {
            driver: "openai".to_string(),
            models: vec![share::config::models::ModelEntryConfig {
                id: "test-model".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let snap = share::config::domain::snapshot::ConfigSnapshot::new(config);
    let mut prompt = String::new();

    append_agent_roles(&mut prompt, Some(&snap), "en");

    assert!(
        prompt.contains("`explorer-mimo`")
            && prompt.contains("[model unavailable: Mimo/mimo-v2.6-flash]"),
        "未注册模型必须标注不可用：{prompt}"
    );
    assert!(
        prompt.contains("`coder-known`")
            && !prompt.contains("[model unavailable: test-provider/test-model]"),
        "已注册模型不得标注不可用：{prompt}"
    );
}

/// ConfigSnapshot.language="zh" 且 lang 参数传 "zh" 时，
/// append_agent_roles 应使用中文 header/footer，prompt 中应出现中文 description。
/// 此测试验证 language 被正确传递给 i18n header/footer（build_static_prompt
/// 从 snap.language() 读取后传入本函数的 lang 参数）。
#[test]
fn test_append_agent_roles_with_snapshot_language_zh() {
    // Arrange — language=zh，验证 lang 参数正确驱动 i18n 文案
    let mut roles = HashMap::new();
    roles.insert(
        "coder".to_string(),
        AgentInstanceConfig {
            description: "编写代码".to_string(),
            ..Default::default()
        },
    );
    let snap = make_snapshot(roles, "zh");
    let mut prompt = String::new();

    // Act
    append_agent_roles(&mut prompt, Some(&snap), "zh");

    // Assert — language=zh 时 role 名与中文 description 应出现
    assert!(prompt.contains("`coder`"), "应包含 role 名 coder");
    assert!(
        prompt.contains("编写代码"),
        "应包含中文 description（language=zh 已正确传递）"
    );
}

#[test]
fn background_task_section_injected_only_when_threshold_enabled() {
    // 默认（阈值>0）：注入后台任务特性说明。
    let enabled = background_tasks_guidance_section("zh");
    assert!(enabled.contains("后台任务"), "zh 段落：{enabled}");
    assert!(enabled.contains("BackgroundTasks"));
    let enabled_en = background_tasks_guidance_section("en");
    assert!(enabled_en.contains("Background tasks"));
    assert!(enabled_en.contains("sequential"));
}

#[test]
fn background_task_section_is_bilingual_and_covers_core_semantics() {
    let zh = background_tasks_guidance_section("zh");
    assert!(zh.contains("自动转后台"), "统一模型");
    assert!(zh.contains("完成会主动通知"), "占位结果语义");
    let en = background_tasks_guidance_section("en");
    assert!(en.contains("moved to the background"));
    assert!(en.contains("will be notified"));
}
