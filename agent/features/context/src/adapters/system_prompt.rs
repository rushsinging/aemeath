//! 系统提示组装（Context-owned，#1863 W4）。
//!
//! 本模块接管原本散在 Runtime 的系统提示拼装职责：静态提示、guidance 正文、
//! agent 名册段、后台进程段、execution discipline 全部在此组装为 `SystemBlock`，
//! Runtime 只负责把窗口交给 provider。`initial_git_context` 与 AGENTS.md 不在
//! 此列——它们是作为普通消息注入的会话材料，不属于系统提示。
//!
//! guidance 加载是磁盘 IO，按 (model, language, reasoning, guidance config)
//! 缓存最近一次结果；每个实际加载的文件经 Hook BC 定义的通知端口上报一次。

use std::sync::{Arc, Mutex};

use share::config::domain::snapshot::ConfigSnapshot;
use share::config::PermissionModeConfig;
use share::i18n::prompt::sections::{agent_roles_footer, agent_roles_header};
use share::i18n::prompt::system::static_system_prompt;
use share::reasoning::ReasoningLevel;

use crate::domain::{ContextRequestData, SystemBlock};

/// 会话级系统提示组装的输入事实（会话启动时冻结一次）。
#[derive(Debug, Clone)]
pub struct SessionPromptInputsData {
    pub workspace_root: std::path::PathBuf,
    pub workspace_is_git: bool,
    pub model_id: String,
    pub reasoning: bool,
    pub language: String,
    pub config_snapshot: ConfigSnapshot,
}

/// guidance 解析缓存键：影响 guidance 文件选择的全部输入。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GuidanceCacheKey {
    model_id: String,
    language: &'static str,
    reasoning: bool,
    guidance_config: Vec<(String, String)>,
}

/// 已解析的 guidance 正文。
#[derive(Debug, Clone, Default)]
struct GuidanceText {
    body: String,
}

/// 系统提示组装器：把会话事实变成 cacheable system blocks。
pub struct SystemPromptAssembly {
    instructions_loaded: Option<Arc<dyn share::instructions::InstructionsLoadedNotifier>>,
    guidance_cache: Mutex<Option<(GuidanceCacheKey, GuidanceText)>>,
}

impl SystemPromptAssembly {
    /// 构造组装器；`instructions_loaded` 为 Hook BC 定义的通知端口，
    /// 由 Runtime 在装配期注入实现。
    pub fn new(
        instructions_loaded: Option<Arc<dyn share::instructions::InstructionsLoadedNotifier>>,
    ) -> Self {
        Self {
            instructions_loaded,
            guidance_cache: Mutex::new(None),
        }
    }

    /// 组装本轮全部 cacheable 系统提示块。
    ///
    /// 顺序即 cache prefix 顺序：静态提示 → agent 名册 → guidance → 后台进程段。
    /// 除静态提示外每段都可能为空（未配置 / 未命中文件），空段不产生 block。
    pub async fn materialize(&self, request: &ContextRequestData) -> Vec<SystemBlock> {
        let language = request.language.as_str();
        let mut blocks = vec![SystemBlock {
            kind: "system_prompt".to_string(),
            content: static_prompt_for(
                &request.workspace_root,
                request.workspace_is_git,
                request.config_snapshot.permission_mode(),
                language,
            ),
            cacheable: true,
            cache_break: false,
        }];

        if let Some(roles) = agent_roles_section_for(&request.config_snapshot, language) {
            blocks.push(SystemBlock {
                kind: "agent_roles".to_string(),
                content: roles,
                cacheable: true,
                cache_break: false,
            });
        }

        let guidance = self
            .guidance_for(
                &request.model_id,
                &request.config_snapshot,
                reasoning_enabled(request.effective_reasoning),
                language,
            )
            .await;
        if !guidance.body.is_empty() {
            blocks.push(SystemBlock {
                kind: "guidance".to_string(),
                content: guidance.body,
                cacheable: true,
                cache_break: false,
            });
        }

        if request.config_snapshot.tool_background_threshold_secs() > 0 {
            blocks.push(SystemBlock {
                kind: "background_processes".to_string(),
                content: background_processes_guidance_section(language).to_string(),
                cacheable: true,
                cache_break: false,
            });
        }

        blocks
    }

    /// 组装会话级静态系统提示全文（块间空行拼接）。
    ///
    /// 与每轮 `materialize` 同源同序，末尾追加 execution discipline。
    /// 会话启动调用一次，产物冻结进 prompt 装配数据。
    pub async fn materialize_session_prompt(&self, inputs: &SessionPromptInputsData) -> String {
        let language = inputs.language.as_str();
        let mut sections: Vec<String> = vec![static_prompt_for(
            &inputs.workspace_root,
            inputs.workspace_is_git,
            inputs.config_snapshot.permission_mode(),
            language,
        )];

        if let Some(roles) = agent_roles_section_for(&inputs.config_snapshot, language) {
            sections.push(roles);
        }
        let guidance = self
            .guidance_for(
                &inputs.model_id,
                &inputs.config_snapshot,
                inputs.reasoning,
                language,
            )
            .await;
        if !guidance.body.is_empty() {
            sections.push(guidance.body);
        }
        if inputs.config_snapshot.tool_background_threshold_secs() > 0 {
            sections.push(background_processes_guidance_section(language).to_string());
        }
        sections.push(
            share::i18n::prompt::discipline::universal_execution_discipline(language).to_string(),
        );
        sections
            .into_iter()
            .filter(|section| !section.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// guidance 正文：按输入指纹缓存；每个实际加载的文件经通知端口上报一次。
    async fn guidance_for(
        &self,
        model_id: &str,
        config_snapshot: &ConfigSnapshot,
        reasoning: bool,
        language: &str,
    ) -> GuidanceText {
        let guidance_config = config_snapshot.models().guidance.clone();
        let mut pairs: Vec<(String, String)> = guidance_config
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        pairs.sort();
        let key = GuidanceCacheKey {
            model_id: model_id.to_string(),
            language: language_static(language),
            reasoning,
            guidance_config: pairs,
        };

        if let Some((cached_key, cached)) = self
            .guidance_cache
            .lock()
            .expect("guidance cache lock poisoned")
            .as_ref()
        {
            if cached_key == &key {
                return cached.clone();
            }
        }

        let (body, sources) = crate::adapters::prompt::guidance::resolve_guidance_with_sources(
            model_id,
            &guidance_config,
            reasoning,
            language,
        );
        if let Some(notifier) = &self.instructions_loaded {
            for path in &sources {
                notifier.instructions_loaded(path, "guidance");
            }
        }
        let text = GuidanceText { body };
        *self
            .guidance_cache
            .lock()
            .expect("guidance cache lock poisoned") = Some((key, text.clone()));
        text
    }
}

/// 是否启用推理：`Off` 之外的所有档位都视为启用（决定 `_reasoning.md` 注入）。
fn reasoning_enabled(level: ReasoningLevel) -> bool {
    level != ReasoningLevel::Off
}

/// 语言只在已知两种之间收敛，用于缓存键稳定比较。
fn language_static(language: &str) -> &'static str {
    match language {
        "zh" => "zh",
        _ => "en",
    }
}

/// 静态提示组装：i18n 正文 + cwd / git 占位替换 + allow_all 模式改写。
pub fn static_prompt_for(
    workspace_root: &std::path::Path,
    workspace_is_git: bool,
    permission_mode: PermissionModeConfig,
    language: &str,
) -> String {
    let cwd_str = workspace_root.to_string_lossy();
    let prompt = static_system_prompt(language)
        .replace("{cwd_str}", &cwd_str)
        .replace("{is_git}", &workspace_is_git.to_string());
    match permission_mode {
        PermissionModeConfig::AllowAll => allow_all_rewrite(prompt, language),
        PermissionModeConfig::Ask | PermissionModeConfig::AutoRead => prompt,
    }
}

/// agent 名册段：枚举 enabled 具名实例，标注角色、描述、模型与可用性。
pub fn agent_roles_section_for(config_snapshot: &ConfigSnapshot, language: &str) -> Option<String> {
    let agents = config_snapshot.agents();
    let merged_roles = agents.merged_roles();
    let role_lines: Vec<String> = agents
        .names
        .iter()
        .filter(|(_, instance)| instance.enabled)
        .map(|(name, instance)| {
            let description = if instance.description.is_empty() {
                merged_roles
                    .get(&instance.role)
                    .map(|role| role.description.as_str())
                    .unwrap_or("")
            } else {
                instance.description.as_str()
            };
            let desc = if description.is_empty() {
                String::new()
            } else {
                format!(": {}", description)
            };
            let model_info = if instance.model.is_empty() {
                String::new()
            } else {
                format!(" (model: {})", instance.model)
            };
            // 可用性口径与 AgentsConfig::resolve_agent 一致：空 model → default_model。
            let effective_model = if instance.model.trim().is_empty() {
                agents.default_model.as_str()
            } else {
                instance.model.as_str()
            };
            let availability = if effective_model.trim().is_empty() {
                " [no model configured]".to_string()
            } else if config_snapshot
                .models()
                .find_model(effective_model)
                .is_none()
            {
                format!(" [model unavailable: {effective_model}]")
            } else {
                String::new()
            };
            format!(
                "- `{}` [{}]{}{}{}",
                name, instance.role, desc, model_info, availability
            )
        })
        .collect();
    if role_lines.is_empty() {
        return None;
    }
    Some(format!(
        "{}{}{}",
        agent_roles_header(language),
        role_lines.join("\n"),
        agent_roles_footer(language)
    ))
}

/// allow_all 模式改写：放宽 workspace 边界表述。
fn allow_all_rewrite(prompt: String, lang: &str) -> String {
    let (workspace_boundary, relative_path_guidance, worktree_guidance) = match lang {
        "zh" => (
            "path_base 是相对路径解析基（相对路径会与 path_base 拼接成绝对路径）。在 allow_all 模式下，workspace_root 仅用于提供上下文信息；路径无需限制在 workspace_root 内。",
            "工作目录内的操作优先使用相对路径；必要时允许使用当前工作区外的绝对路径。",
            "当 EnterWorktree 或 ExitWorktree 的工具结果返回新的 path_base/workspace_root 时，应使用最新的工作区上下文。",
        ),
        _ => (
            "path_base is the base for resolving relative paths (relative paths are joined to path_base to form absolute paths). In allow-all mode, workspace_root is informational: paths are not required to stay within workspace_root.",
            "Prefer relative paths for operations in the working directory. Absolute paths may be used outside the current workspace when required.",
            "When EnterWorktree or ExitWorktree returns a new path_base/workspace_root in its tool result, use that latest tool result as the current workspace context.",
        ),
    };

    prompt
        .replace(
            "path_base is the base for resolving relative paths; workspace_root is the safety boundary.",
            workspace_boundary,
        )
        .replace(
            "path_base 是相对路径解析基；workspace_root 是安全边界。",
            workspace_boundary,
        )
        .replace(
            "Prefer relative paths. Absolute paths must remain inside the current workspace.",
            relative_path_guidance,
        )
        .replace(
            "优先使用相对路径；绝对路径必须位于当前 workspace 内。",
            relative_path_guidance,
        )
        .replace(
            "After EnterWorktree or ExitWorktree, use the latest path_base/workspace_root returned by the tool and do not reuse paths from another checkout.",
            worktree_guidance,
        )
        .replace(
            "EnterWorktree 或 ExitWorktree 后，以工具返回的最新 path_base/workspace_root 为准，禁止复用其他 checkout 的路径。",
            worktree_guidance,
        )
}

/// 后台进程特性 system prompt 段（#252 D6 / 设计 §8，双语）。
pub(crate) fn background_processes_guidance_section(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            "# 后台进程\n\
            超过前台等待阈值（当前会话已启用）的 tool call 会自动转后台运行：你会先收到一条占位结果（标注任务已转后台、非终态，并附任务日志文件路径），任务完成会主动通知你并回注结果，届时可继续处理。\n\
            - 转后台不等于完成：不要重复派发同一任务；先处理其他可并发的工作，或等待完成通知唤醒后继续。\n\
            - 优先等待完成通知；NEVER 对同一 task_id 忙等轮询 BackgroundProcessStatus。\n\
            - 确需中途进度时用 BackgroundProcessLogs（task_id + 增量游标，全量字节无截断）或直接读占位里的日志文件路径；不要为「等完成」反复调用 Logs。\n\
            - BackgroundProcessStatus 仅偶发一次用于确认（例如怀疑通知丢失）；用 BackgroundProcessList 查看列表，用 BackgroundProcessWatch 做前台短等。\n\
            - 只有当多个任务确实互不依赖时才并行；有依赖关系时按顺序执行。"
        }
        _ => {
            "# Background processes\n\
            Tool calls exceeding the foreground wait threshold (enabled in this session) are moved to the background automatically: you first receive a placeholder result (marked as moved to the background, non-terminal, with the task log file path), and you will be notified when the task completes with its result injected back.\n\
            - Moved to background does not mean done: do not re-dispatch the same task; work on other concurrent work instead, or wait for the completion notification to wake you.\n\
            - Prefer waiting for the completion notification; NEVER busy-poll BackgroundProcessStatus for the same task_id.\n\
            - For mid-run progress use BackgroundProcessLogs (task_id + incremental cursor, full bytes without truncation) or read the log file path from the placeholder; do not repeatedly call Logs just to wait for completion.\n\
            - BackgroundProcessStatus is only for an occasional confirmation (e.g. suspecting a lost notification); use BackgroundProcessList for the task list and BackgroundProcessWatch for a foreground short wait.\n\
            - Only parallelize tasks that are genuinely independent; run dependent tasks sequentially."
        }
    }
}
