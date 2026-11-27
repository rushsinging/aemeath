//! 从 TaskTrace 合成可复用 skill。
//!
//! LLM 负责语义归纳；宿主负责候选筛选、脱敏、结构校验、去重、快照和写入。

use crate::application::ComponentRegistry;
use crate::constants::EVIDENCE_EXCERPT_BOUND;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::errors::HarnessError;
use crate::domain::events::RsiCycleEvent;
use crate::domain::improvement::{complete_with_usage_event, RsiLlmPort};
use crate::domain::lesson::EvidenceBinding;
use crate::domain::redaction::redact_sensitive_text;
use crate::domain::task_trace::TaskTrace;
use crate::domain::versioning::ComponentVersionStore;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
enum SkillSynthesisDraft {
    Create {
        name: String,
        description: String,
        #[serde(default)]
        when_to_use: Vec<String>,
        #[serde(default)]
        steps: Vec<String>,
        #[serde(default)]
        verification: Vec<String>,
        #[serde(default)]
        pitfalls: Vec<String>,
        evidence_refs: Vec<String>,
        #[serde(default)]
        reason: String,
    },
    NoChange {
        #[serde(default)]
        reason: String,
    },
}

fn valid_skill_name(name: &str) -> bool {
    let len = name.chars().count();
    (2..=50).contains(&len)
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

fn candidate(trace: &TaskTrace) -> bool {
    trace.candidate_score() >= 8
        && !trace.actions.iter().any(|action| {
            action.command.as_deref().is_some_and(|command| {
                let lower = command.to_ascii_lowercase();
                lower.contains("sk-")
                    || lower.contains("bearer ")
                    || lower.contains("password")
                    || lower.contains("private key")
            })
        })
}

fn trace_prompt(trace: &TaskTrace, existing: &[(String, String)]) -> String {
    let goal = redact_sensitive_text(&trace.user_goal);
    let mut selected_indices = std::collections::BTreeSet::new();
    for index in 0..trace.actions.len().min(4) {
        selected_indices.insert(index);
    }
    for index in trace.actions.len().saturating_sub(4)..trace.actions.len() {
        selected_indices.insert(index);
    }
    for (index, action) in trace.actions.iter().enumerate() {
        if action.kind != crate::domain::task_trace::TaskActionKind::Other
            && selected_indices.len() < 40
        {
            selected_indices.insert(index);
        }
    }
    let actions = trace
        .actions
        .iter()
        .enumerate()
        .filter(|(index, _)| selected_indices.contains(index))
        .map(|(_, action)| {
            format!(
                "- {:?} / {} / tool_success={:?} / target={} / command={} / excerpt={}",
                action.kind,
                action.tool,
                action.success,
                action.target_path.as_deref().unwrap_or(""),
                action
                    .command
                    .as_deref()
                    .map(|command| {
                        redact_sensitive_text(command)
                            .chars()
                            .take(300)
                            .collect::<String>()
                    })
                    .unwrap_or_default(),
                action
                    .excerpt
                    .as_deref()
                    .map(|excerpt| {
                        redact_sensitive_text(excerpt)
                            .chars()
                            .take(160)
                            .collect::<String>()
                    })
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let verification = trace
        .verification
        .iter()
        .map(|fact| {
            format!(
                "- tool_success={:?} / command={} / excerpt={}",
                fact.success,
                redact_sensitive_text(&fact.command),
                fact.excerpt
                    .as_deref()
                    .map(|excerpt| {
                        redact_sensitive_text(excerpt)
                            .chars()
                            .take(160)
                            .collect::<String>()
                    })
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let sources = trace
        .sources
        .iter()
        .take(24)
        .enumerate()
        .map(|(index, source)| {
            format!(
                "E{index}: {}/{}/{}",
                source.session_id, source.run_id, source.step_id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let existing_text = existing
        .iter()
        .map(|(name, raw)| {
            format!(
                "- {name}: {}",
                redact_sensitive_text(raw)
                    .chars()
                    .take(240)
                    .collect::<String>()
                    .replace('\n', " ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"你是技能归纳器。请把一个真实任务轨迹归纳成可复用的 skill，而不是复述工具调用。

现有 skill（名称与完整原文）：
{existing_text}

任务轨迹：
- 用户目标：{goal}
- 修改文件：{files}
- 动作（按顺序）：{actions}
- 验证事实：{verification}
- 终态：{outcome}
- 可选证据来源（session/run/step）：
{sources}

只有任务满足以下条件才创建：
1. 有明确目标和任务边界；
2. 有可复用的步骤、验证和停止条件；
3. 不是一次性路径、凭据、临时编号或具体用户数据；
4. 与现有 skill 不重复；相似度仅是召回线索，必须由你比较目标、适用范围、步骤和约束后裁决；特化或互补 skill 应保留；
5. 不把 Bash/后台等待等噪声动作当作核心步骤；
6. 不得扩大源任务授权：读取 issue 不代表可以修改 issue、merge 或执行其他写操作；只保留源任务明确授权且实际使用的能力；
7. tool_success 只表示工具调用是否完成，不等于任务成功；None 表示未知，不得推断成功。

只输出一个 JSON 对象：
{{"decision":"create","name":"lowercase-hyphen-name","description":"<非空描述>","when_to_use":["<触发条件>"],"steps":["<步骤1>","<步骤2>"],"verification":["<验收1>"],"pitfalls":["<陷阱1>"],"evidence_refs":["E0"],"reason":"<一句话>"}}
或：
{{"decision":"no_change","reason":"<一句话>"}}
create 的 evidence_refs 必须非空，只能引用上面真实列出的短 ref，且不得重复。name 只能使用小写字母、数字、连字符，2–50 字符；不要输出 JSON 以外的文字。"#,
        existing_text = existing_text,
        goal = goal.chars().take(1200).collect::<String>(),
        files = redact_sensitive_text(&trace.changed_files.join(", "))
            .chars()
            .take(600)
            .collect::<String>(),
        actions = actions,
        verification = verification,
        outcome = trace.outcome,
        sources = sources,
    )
}

fn render_body(
    name: &str,
    description: &str,
    when_to_use: &[String],
    steps: &[String],
    verification: &[String],
    pitfalls: &[String],
) -> String {
    let list = |title: &str, values: &[String]| {
        format!(
            "## {title}\n\n{}\n",
            values
                .iter()
                .map(|value| format!("- {value}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let description = serde_json::to_string(description)
        .expect("JSON string serialization is infallible for String");
    format!(
        "---\ndescription: {description}\n---\n# {name}\n\n{}\n{}\n{}\n{}\n",
        list("使用场景", when_to_use),
        list("步骤", steps),
        list("验收", verification),
        list("常见失败", pitfalls),
    )
}

#[derive(Debug, Deserialize)]
struct SkillDuplicateVerdict {
    verdict: String,
    #[serde(default)]
    reason: String,
}

async fn duplicate_skill_reason(
    llm: &dyn RsiLlmPort,
    body: &str,
    existing: &[(String, String)],
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
) -> Result<Option<String>, HarnessError> {
    let mut candidates = existing
        .iter()
        .map(|(name, raw)| {
            (
                name,
                raw,
                share::memory::dedup::jaccard_similarity(body, raw),
            )
        })
        .filter(|(_, _, score)| *score >= 0.04)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.2.total_cmp(&left.2));
    candidates.truncate(6);
    if candidates.is_empty() {
        return Ok(None);
    }
    let candidate_text = candidates
        .iter()
        .map(|(name, raw, score)| {
            format!(
                "=== {name} / 召回 {score:.3} ===\n{}",
                redact_sensitive_text(raw)
                    .chars()
                    .take(1800)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let prompt = format!(
        "判断新 skill 与已有 skill 的关系。duplicate 表示目标、步骤与约束基本等价；specialization 表示保留项目/场景差异；distinct 表示不同能力。\n\n新 skill：\n{body}\n\n已有候选：\n{candidate_text}\n\n只输出 JSON：{{\"verdict\":\"duplicate|specialization|distinct\",\"reason\":\"...\"}}"
    );
    let completion = complete_with_usage_event(llm, &prompt, "skill_dedup", 0, recorder).await?;
    let verdict: SkillDuplicateVerdict =
        crate::domain::improvement::parse_first_json_object(&completion.text, "skill_dedup")?;
    if verdict.verdict == "duplicate" {
        Ok(Some(verdict.reason))
    } else if matches!(verdict.verdict.as_str(), "distinct" | "specialization") {
        Ok(None)
    } else {
        Err(HarnessError::invalid(
            crate::constants::ERROR_DOMAIN,
            "skill 去重裁决值非法，fail-closed",
        ))
    }
}

fn reject_synthesis(
    preview_lines: &mut Vec<String>,
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    message: String,
) {
    preview_lines.push(format!("  [skill-trace] {message}"));
    if let Some(recorder) = recorder {
        recorder.record(&RsiCycleEvent::ProposalRejected {
            block_index: 0,
            stage: "skill_synthesis".to_string(),
            message,
        });
    }
}

fn resolve_evidence_refs(
    evidence_refs: &[String],
    trace: &TaskTrace,
) -> Result<Vec<(String, crate::domain::evidence::EvidenceSourceRef)>, String> {
    if evidence_refs.is_empty() {
        return Err("evidence_refs 为空".to_string());
    }
    let mut seen = Vec::with_capacity(evidence_refs.len());
    let mut resolved = Vec::with_capacity(evidence_refs.len());
    for evidence_ref in evidence_refs {
        let Some(index) = evidence_ref
            .strip_prefix('E')
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return Err(format!("未知 evidence_ref：{evidence_ref}"));
        };
        if index >= trace.sources.len() {
            return Err(format!("未知 evidence_ref：{evidence_ref}"));
        }
        if seen.contains(&index) {
            return Err(format!("重复 evidence_ref：{evidence_ref}"));
        }
        seen.push(index);
        resolved.push((evidence_ref.clone(), trace.sources[index].clone()));
    }
    Ok(resolved)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillDisposition {
    ReuseExisting,
    ReviseExisting,
    CreateNew,
    NoAction,
}

#[derive(Debug, Deserialize)]
struct SkillDispositionDraft {
    decision: String,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    reason: String,
}

async fn decide_skill_disposition(
    llm: &dyn RsiLlmPort,
    trace: &TaskTrace,
    existing: &[(String, String)],
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
) -> Result<(SkillDisposition, Option<String>, String), HarnessError> {
    if existing.is_empty() {
        return Ok((SkillDisposition::CreateNew, None, String::new()));
    }
    let names = existing
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut candidates = existing
        .iter()
        .map(|(name, raw)| {
            (
                name,
                raw,
                share::memory::dedup::jaccard_similarity(&trace.user_goal, raw),
            )
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.2.total_cmp(&left.2));
    let candidates = candidates
        .iter()
        .take(8)
        .map(|(name, raw, _)| {
            format!(
                "=== {name} ===\n{}",
                redact_sensitive_text(raw)
                    .chars()
                    .take(1200)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let action_summary = trace
        .actions
        .iter()
        .filter(|action| action.kind != crate::domain::task_trace::TaskActionKind::Other)
        .take(12)
        .map(|action| {
            format!(
                "- {:?} {} {}",
                action.kind,
                action.tool,
                action
                    .command
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "先判断已有 skill 能否承接任务，再决定是否新建。\n任务目标：{}\n修改文件：{}\n任务动作：\n{action_summary}\n已有 skill 名称：{names}\n相关已有 skill：\n{candidates}\n\n决策规则：\n- reuse_existing：已有 skill 可直接承接，不写文件；\n- revise_existing：已有 skill 目标匹配但步骤需要补充/修正，走最小 patch；\n- create_new：没有合适已有 skill，才新建；\n- no_action：证据不足，不创建也不修改。\n优先复用，其次修订，最后新建。skill 字段在 reuse/revise 时必须精确引用一个已有名称。\n只输出 JSON：{{\"decision\":\"reuse_existing|revise_existing|create_new|no_action\",\"skill\":\"name-or-null\",\"reason\":\"...\"}}",
        redact_sensitive_text(&trace.user_goal).chars().take(1000).collect::<String>(),
        redact_sensitive_text(&trace.changed_files.join(", ")).chars().take(400).collect::<String>(),
    );
    let mut draft = None;
    for attempt in 0..2 {
        let completion = complete_with_usage_event(
            llm,
            &prompt,
            if attempt == 0 {
                "skill_disposition"
            } else {
                "skill_disposition_retry"
            },
            0,
            recorder,
        )
        .await?;
        if let Ok(value) = crate::domain::improvement::parse_first_json_object::<
            SkillDispositionDraft,
        >(&completion.text, "skill_disposition")
        {
            draft = Some(value);
            break;
        }
    }
    let draft = draft.ok_or_else(|| {
        HarnessError::invalid(
            crate::constants::ERROR_DOMAIN,
            "skill 承接判定解析失败（已重试）",
        )
    })?;
    let disposition = match draft.decision.as_str() {
        "reuse_existing" => SkillDisposition::ReuseExisting,
        "revise_existing" => SkillDisposition::ReviseExisting,
        "create_new" | "create" => SkillDisposition::CreateNew,
        "no_action" => SkillDisposition::NoAction,
        _ => {
            return Err(HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                "skill 承接判定值非法",
            ));
        }
    };
    if matches!(
        disposition,
        SkillDisposition::ReuseExisting | SkillDisposition::ReviseExisting
    ) {
        let Some(name) = draft.skill.as_deref() else {
            return Err(HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                "reuse/revise 必须指定已有 skill",
            ));
        };
        if !existing
            .iter()
            .any(|(existing_name, _)| existing_name == name)
        {
            return Err(HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                "承接判定引用了不存在的 skill",
            ));
        }
    }
    Ok((disposition, draft.skill, draft.reason))
}

#[derive(Debug, Deserialize)]
struct SkillRevisionDraft {
    decision: String,
    #[serde(default)]
    evidence_refs: Vec<String>,
    #[serde(default)]
    reason: String,
    operations: Vec<crate::domain::changeset::PatchOperation>,
}

struct SkillRevisionContext<'a> {
    dry_run: bool,
    recorder: Option<&'a crate::domain::events::RsiEventRecorder>,
    version_store: Option<&'a ComponentVersionStore>,
    cycle_id: &'a str,
}

async fn revise_skill_from_trace(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    trace: &TaskTrace,
    existing_name: &str,
    preview_lines: &mut Vec<String>,
    context: SkillRevisionContext<'_>,
) -> Result<Option<crate::domain::task::AdoptionRecord>, HarnessError> {
    let id = ComponentId {
        kind: ComponentKind::Skill,
        name: ComponentName::new(existing_name),
    };
    let raw = registry.read_raw(&id)?;
    let prompt = format!(
        "你是最小修订器。根据任务轨迹修订已有 skill，保留无关原文与授权边界，NEVER 整篇重写。\n任务目标：{}\n已有 skill 原文：\n{}\n\n只输出 JSON：{{\"decision\":\"revise\",\"evidence_refs\":[\"E0\"],\"reason\":\"...\",\"operations\":[{{\"op\":\"replace_exact\",\"old_string\":\"...\",\"new_string\":\"...\"}}]}}\noperations 可为空；锚点必须在原文中唯一。",
        redact_sensitive_text(&trace.user_goal).chars().take(1000).collect::<String>(),
        redact_sensitive_text(&raw),
    );
    let mut draft = None;
    for attempt in 0..2 {
        let completion = complete_with_usage_event(
            llm,
            &prompt,
            if attempt == 0 {
                "skill_revision"
            } else {
                "skill_revision_retry"
            },
            0,
            context.recorder,
        )
        .await?;
        if let Ok(value) = crate::domain::improvement::parse_first_json_object::<SkillRevisionDraft>(
            &completion.text,
            "skill_revision",
        ) {
            draft = Some(value);
            break;
        }
    }
    let Some(draft) = draft else {
        reject_synthesis(
            preview_lines,
            context.recorder,
            "修订草案解析失败（已重试）".to_string(),
        );
        return Ok(None);
    };
    if draft.decision != "revise" || draft.operations.is_empty() {
        preview_lines.push(format!("  [skill-disposition] 无需修订 {id}"));
        return Ok(None);
    }
    let resolved_sources = match resolve_evidence_refs(&draft.evidence_refs, trace) {
        Ok(sources) => sources,
        Err(message) => {
            reject_synthesis(preview_lines, context.recorder, message);
            return Ok(None);
        }
    };
    let patched = match crate::domain::changeset::apply_patch_operations(&raw, &draft.operations) {
        Ok(body) => body,
        Err(error) => {
            reject_synthesis(
                preview_lines,
                context.recorder,
                format!("修订 patch 失败：{}", error.message()),
            );
            return Ok(None);
        }
    };
    let content = ComponentContent {
        frontmatter: None,
        body: patched.clone(),
    };
    if let Err(error) = registry.validate(&id, &content) {
        reject_synthesis(
            preview_lines,
            context.recorder,
            format!("修订校验失败：{}", error.message()),
        );
        return Ok(None);
    }
    if !context.dry_run {
        let Some(store) = context.version_store else {
            reject_synthesis(
                preview_lines,
                context.recorder,
                "修订缺少版本快照，拒绝写入".to_string(),
            );
            return Ok(None);
        };
        if let Err(error) = store.capture(
            registry,
            &id,
            &format!("{}-skill-revision", context.cycle_id),
            "patch",
        ) {
            reject_synthesis(
                preview_lines,
                context.recorder,
                format!("修订快照失败：{}", error.message()),
            );
            return Ok(None);
        }
    }
    if let Err(error) = registry.write_raw(&id, &patched) {
        reject_synthesis(
            preview_lines,
            context.recorder,
            format!("修订写入失败：{}", error.message()),
        );
        return Ok(None);
    }
    if let Some(recorder) = context.recorder {
        recorder.record(&RsiCycleEvent::EditApplied {
            component: id.to_string(),
            action: "revise".to_string(),
            old_chars: raw.chars().count(),
            new_chars: patched.chars().count(),
        });
    }
    preview_lines.push(format!(
        "{}[skill-disposition] revise {id}\n{patched}",
        if context.dry_run { "[dry-run]" } else { "" }
    ));
    let evidence = resolved_sources
        .into_iter()
        .map(|(evidence_ref, source)| EvidenceBinding {
            run_ref: context.cycle_id.to_string(),
            signal: crate::domain::evidence::SignalKind::CommonAction,
            excerpt: crate::domain::evidence::truncate_chars(
                &trace.user_goal,
                EVIDENCE_EXCERPT_BOUND,
            )
            .unwrap_or_default(),
            signal_id: format!("task_trace:{}:{evidence_ref}", trace.trace_id()),
            source: Some(source),
        })
        .collect();
    Ok(Some(crate::domain::task::AdoptionRecord {
        id,
        reason: draft.reason,
        evidence,
    }))
}

pub async fn synthesize_skill_from_trace(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    trace: &TaskTrace,
    dry_run: bool,
    preview_lines: &mut Vec<String>,
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    version_store: Option<&ComponentVersionStore>,
    cycle_id: &str,
) -> Result<Option<crate::domain::task::AdoptionRecord>, HarnessError> {
    if trace.user_goal.trim().is_empty() {
        reject_synthesis(
            preview_lines,
            recorder,
            format!("候选轨迹缺少用户目标：{}", trace.trace_id()),
        );
        return Ok(None);
    }
    let existing = registry
        .list()
        .into_iter()
        .filter(|id| id.kind == ComponentKind::Skill)
        .filter_map(|id| {
            registry
                .read_raw(&id)
                .ok()
                .map(|raw| (id.name.as_str().to_string(), raw))
        })
        .collect::<Vec<_>>();
    let (disposition, existing_name, reason) =
        match decide_skill_disposition(llm, trace, &existing, recorder).await {
            Ok(decision) => decision,
            Err(error) => {
                reject_synthesis(
                    preview_lines,
                    recorder,
                    format!("承接判定失败：{}", error.message()),
                );
                return Ok(None);
            }
        };
    if disposition == SkillDisposition::CreateNew && !candidate(trace) {
        reject_synthesis(
            preview_lines,
            recorder,
            format!("新建候选轨迹不合格：{}", trace.trace_id()),
        );
        return Ok(None);
    }
    preview_lines.push(format!(
        "  [skill-disposition] {:?} {}：{reason}",
        disposition,
        existing_name.as_deref().unwrap_or("")
    ));
    match disposition {
        SkillDisposition::ReuseExisting => {
            return Ok(None);
        }
        SkillDisposition::ReviseExisting => {
            let Some(existing_name) = existing_name else {
                return Ok(None);
            };
            return revise_skill_from_trace(
                registry,
                llm,
                trace,
                &existing_name,
                preview_lines,
                SkillRevisionContext {
                    dry_run,
                    recorder,
                    version_store,
                    cycle_id,
                },
            )
            .await;
        }
        SkillDisposition::NoAction => {
            return Ok(None);
        }
        SkillDisposition::CreateNew => {}
    }
    let prompt = trace_prompt(trace, &existing);
    let mut draft = None;
    for attempt in 0..2 {
        let completion =
            complete_with_usage_event(llm, &prompt, "skill_synthesis", 0, recorder).await?;
        match crate::domain::improvement::parse_first_json_object(
            &completion.text,
            "skill_synthesis",
        ) {
            Ok(parsed) => {
                draft = Some(parsed);
                break;
            }
            Err(error) if attempt == 0 => {
                preview_lines.push(format!(
                    "  [skill-trace] 草案解析失败，重试一次：{}",
                    error.message()
                ));
            }
            Err(error) => {
                reject_synthesis(
                    preview_lines,
                    recorder,
                    format!("草案解析失败（已重试）：{}", error.message()),
                );
                return Ok(None);
            }
        }
    }
    let Some(draft) = draft else {
        return Ok(None);
    };
    let SkillSynthesisDraft::Create {
        name,
        description,
        when_to_use,
        steps,
        verification,
        pitfalls,
        evidence_refs,
        reason,
    } = draft
    else {
        if let SkillSynthesisDraft::NoChange { reason } = draft {
            reject_synthesis(preview_lines, recorder, format!("no_change：{reason}"));
        }
        return Ok(None);
    };
    if !valid_skill_name(&name)
        || description.trim().is_empty()
        || when_to_use.is_empty()
        || steps.len() < 3
        || verification.is_empty()
    {
        reject_synthesis(preview_lines, recorder, format!("结构校验失败：{name}"));
        return Ok(None);
    }
    let resolved_sources = match resolve_evidence_refs(&evidence_refs, trace) {
        Ok(sources) => sources,
        Err(message) => {
            reject_synthesis(preview_lines, recorder, message);
            return Ok(None);
        }
    };
    if existing.iter().any(|(old, _)| old == &name) {
        reject_synthesis(
            preview_lines,
            recorder,
            format!("同名 skill 已存在：{name}"),
        );
        return Ok(None);
    }
    let body = render_body(
        &name,
        &description,
        &when_to_use,
        &steps,
        &verification,
        &pitfalls,
    );
    let lower = body.to_ascii_lowercase();
    if [
        "sk-",
        "bearer ",
        "api_key",
        "password",
        "private key",
        "/users/",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        reject_synthesis(
            preview_lines,
            recorder,
            format!("含路径/凭据，拒绝：{name}"),
        );
        return Ok(None);
    }
    match duplicate_skill_reason(llm, &body, &existing, recorder).await {
        Ok(Some(reason)) => {
            reject_synthesis(
                preview_lines,
                recorder,
                format!("与已有 skill 语义重复，拒绝：{reason}"),
            );
            return Ok(None);
        }
        Ok(None) => {}
        Err(error) => {
            reject_synthesis(
                preview_lines,
                recorder,
                format!("skill 去重失败，fail-closed：{}", error.message()),
            );
            return Ok(None);
        }
    }
    let id = ComponentId {
        kind: ComponentKind::Skill,
        name: ComponentName::new(name.clone()),
    };
    let content = ComponentContent {
        frontmatter: None,
        body,
    };
    registry.validate(&id, &content)?;
    preview_lines.push(format!(
        "{}[skill-trace] create {id}：{reason}\n{}",
        if dry_run { "[dry-run]" } else { "  " },
        content.body
    ));
    if !dry_run {
        let store = version_store.ok_or_else(|| {
            HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                "正式 skill 写入缺少 snapshot store，拒绝写入",
            )
        })?;
        store.capture(registry, &id, &format!("{cycle_id}-skill-trace"), "upsert")?;
    }
    // dry-run 传入 VirtualComponentRegistry：同一写入路径只进入内存 overlay，
    // 后续 Compose 可读取完整终态；正式路径已在写入前建立 tombstone 快照。
    registry.write(&id, &content)?;
    if let Some(recorder) = recorder {
        recorder.record(&RsiCycleEvent::EditApplied {
            component: id.to_string(),
            action: if dry_run { "dry_run_create" } else { "create" }.to_string(),
            old_chars: 0,
            new_chars: content.body.chars().count(),
        });
    }
    let evidence = resolved_sources
        .into_iter()
        .map(|(evidence_ref, source)| EvidenceBinding {
            run_ref: cycle_id.to_string(),
            signal: crate::domain::evidence::SignalKind::CommonAction,
            excerpt: crate::domain::evidence::truncate_chars(
                &trace.user_goal,
                EVIDENCE_EXCERPT_BOUND,
            )
            .unwrap_or_default(),
            signal_id: format!("task_trace:{}:{evidence_ref}", trace.trace_id()),
            source: Some(source),
        })
        .collect();
    Ok(Some(crate::domain::task::AdoptionRecord {
        id,
        reason: if reason.is_empty() {
            format!("从任务轨迹 {} 生成 skill", trace.trace_id())
        } else {
            reason
        },
        evidence,
    }))
}

/// 只选择一个候选，避免每周期批量创建 skill。
/// 低分任务也可进入承接判定：它们可能只需复用/no_action，不能直接当新建证据。
pub fn select_skill_candidate(traces: &[TaskTrace]) -> Option<&TaskTrace> {
    traces
        .iter()
        .filter(|trace| !trace.user_goal.trim().is_empty())
        .max_by_key(|trace| trace.candidate_score())
}
