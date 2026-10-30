//! RSI signal → component 受限路由。
//!
//! Router 是宿主侧策略，不是 RSI 可编辑组件：它先根据证据与候选组件生成
//! `Exact / Choose / Unresolved`，诊断与执行阶段 NEVER 自由选择路由外组件。

use crate::constants::ROUTABLE_SIGNAL_KINDS;
use crate::domain::changeset::ChangeSet;
use crate::domain::component::{ComponentId, ComponentKind};
use crate::domain::evidence::{EvidenceBundle, EvidenceSignal, SignalKind};

/// 组件职责面（用于路由解释与优先级，不进入组件正文）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentScope {
    GlobalOverlay,
    Guardrails,
    ToolGuidance,
    Skill,
    SubAgentsConfig,
}

/// 单个组件允许承接的 signal 形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalRoutePattern {
    /// 内置工具的执行纪律信号（Bash/Read/Edit/Grep/TaskUpdate 等）。
    BuiltinTool(SignalKind),
    /// 精确 skill 名匹配。
    Skill { name: String },
    /// Agent/subagent 配置或派发信号。
    Agent,
}

/// 宿主内置组件路由描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentRouteProfile {
    pub id: ComponentId,
    pub scope: ComponentScope,
    pub allowed_signals: Vec<SignalRoutePattern>,
    /// 数值越大越具体；router 保留更具体候选在前。
    pub priority: u8,
}

/// 路由补充上下文（来自 session step 的有界回读）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteContext {
    pub skill_name: Option<String>,
}

/// prompt 使用的短证据事实：模型只见 E0/E1，完整 signal id 由 runtime 映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEvidenceFact {
    pub evidence_ref: String,
    pub signal: EvidenceSignal,
}

/// 按 prompt 渲染优先级生成短证据引用；顺序与 evidence 渲染保持一致。
pub(crate) fn route_evidence_facts(evidence: &EvidenceBundle) -> Vec<RouteEvidenceFact> {
    let mut signals = evidence.signals.clone();
    signals.sort_by_key(|signal| signal.kind.render_priority());
    signals
        .into_iter()
        .enumerate()
        .map(|(index, signal)| RouteEvidenceFact {
            evidence_ref: format!("E{index}"),
            signal,
        })
        .collect()
}

impl RouteContext {
    /// 从有界 session 上下文中提取 Skill 名。只做最小 JSON 字符串字段
    /// 识别，NEVER 解析整段上下文或要求上下文是完整 JSON。
    pub fn from_context_excerpt(text: &str) -> Self {
        Self {
            skill_name: extract_json_string_field(text, "skill"),
        }
    }
}

fn extract_json_string_field(text: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\"");
    let field_start = text.find(&marker)? + marker.len();
    let colon = text[field_start..].find(':')? + field_start;
    let quote = text[colon..].find('"')? + colon + 1;
    let mut value = String::new();
    let mut escaped = false;
    for character in text[quote..].chars() {
        if escaped {
            value.push(character);
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '"' => return Some(value),
            _ => value.push(character),
        }
    }
    None
}

/// 单条 signal 的路由决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteDecision {
    Exact {
        component: ComponentId,
        reason: String,
    },
    Choose {
        candidates: Vec<ComponentId>,
        reason: String,
    },
    Unresolved {
        reason: String,
    },
}

pub(crate) fn signal_tool(signal: &EvidenceSignal) -> &str {
    signal.signal_id.split(':').nth(1).unwrap_or("")
}

fn is_builtin_tool(tool: &str) -> bool {
    !matches!(tool, "" | "Skill" | "Agent")
}

fn profile_for(id: &ComponentId) -> ComponentRouteProfile {
    match id.kind {
        ComponentKind::ToolGuidance => ComponentRouteProfile {
            id: id.clone(),
            scope: ComponentScope::ToolGuidance,
            allowed_signals: ROUTABLE_SIGNAL_KINDS
                .into_iter()
                .map(SignalRoutePattern::BuiltinTool)
                .collect(),
            priority: 50,
        },
        ComponentKind::Guardrails => ComponentRouteProfile {
            id: id.clone(),
            scope: ComponentScope::Guardrails,
            allowed_signals: ROUTABLE_SIGNAL_KINDS
                .into_iter()
                .map(SignalRoutePattern::BuiltinTool)
                .collect(),
            priority: 40,
        },
        ComponentKind::SystemPromptOverlay => ComponentRouteProfile {
            id: id.clone(),
            scope: ComponentScope::GlobalOverlay,
            allowed_signals: ROUTABLE_SIGNAL_KINDS
                .into_iter()
                .map(SignalRoutePattern::BuiltinTool)
                .collect(),
            priority: 30,
        },
        ComponentKind::Skill => ComponentRouteProfile {
            id: id.clone(),
            scope: ComponentScope::Skill,
            allowed_signals: vec![SignalRoutePattern::Skill {
                name: id.name.as_str().to_string(),
            }],
            priority: 100,
        },
        ComponentKind::SubAgents => ComponentRouteProfile {
            id: id.clone(),
            scope: ComponentScope::SubAgentsConfig,
            allowed_signals: vec![SignalRoutePattern::Agent],
            priority: 90,
        },
        ComponentKind::MemoryView | ComponentKind::Plugin => ComponentRouteProfile {
            id: id.clone(),
            scope: ComponentScope::Guardrails,
            allowed_signals: Vec::new(),
            priority: 0,
        },
    }
}

fn profile_allows(
    profile: &ComponentRouteProfile,
    signal: &EvidenceSignal,
    context: &RouteContext,
) -> bool {
    let tool = signal_tool(signal);
    profile.allowed_signals.iter().any(|pattern| match pattern {
        SignalRoutePattern::BuiltinTool(expected_kind) => {
            *expected_kind == signal.kind && is_builtin_tool(tool)
        }
        SignalRoutePattern::Skill { name } => {
            tool == "Skill"
                && signal.kind != SignalKind::RunOutcome
                && context.skill_name.as_deref() == Some(name.as_str())
        }
        SignalRoutePattern::Agent => tool == "Agent" && signal.kind != SignalKind::RunOutcome,
    })
}

/// 为一条 signal 生成受限组件决策。候选顺序按 profile priority 稳定排序。
pub fn route_signal(
    signal: &EvidenceSignal,
    candidates: &[ComponentId],
    context: &RouteContext,
) -> RouteDecision {
    if matches!(
        signal.kind,
        SignalKind::RunOutcome | SignalKind::CycleObservation
    ) {
        return RouteDecision::Unresolved {
            reason: "run 终态/周期观察没有可信托管组件".to_string(),
        };
    }
    if !ROUTABLE_SIGNAL_KINDS.contains(&signal.kind) {
        return RouteDecision::Unresolved {
            reason: format!("signal kind {} 不在可路由面", signal.kind.as_str()),
        };
    }

    let tool = signal_tool(signal).to_string();
    // 内置工具纪律的主职责面是 tool-guidance/guardrails；overlay 只能在
    // 主候选存在时作为低优先级全局候选参与选择。主候选缺失时 MUST
    // unresolved，NEVER 让 always-on overlay 独自兜底而持续膨胀。
    if is_builtin_tool(&tool)
        && !candidates.iter().any(|candidate| {
            matches!(
                candidate.kind,
                ComponentKind::ToolGuidance | ComponentKind::Guardrails
            )
        })
    {
        return RouteDecision::Unresolved {
            reason: "缺少 tool-guidance/guardrails 主候选，内置工具纪律 NEVER 兜底到 overlay"
                .to_string(),
        };
    }
    let mut routed: Vec<(ComponentRouteProfile, String)> = candidates
        .iter()
        .filter_map(|id| {
            let profile = profile_for(id);
            if profile_allows(&profile, signal, context) {
                Some((profile, id.to_string()))
            } else {
                None
            }
        })
        .collect();
    routed.sort_by_key(|(profile, _)| std::cmp::Reverse(profile.priority));

    if tool == "Skill" {
        return match routed.first() {
            Some((profile, _)) => RouteDecision::Exact {
                component: profile.id.clone(),
                reason: format!("Skill 证据精确匹配 {}", profile.id.name),
            },
            None => RouteDecision::Unresolved {
                reason: "Skill 证据缺少精确 skill 名，NEVER 兜底到全局组件".to_string(),
            },
        };
    }

    let ids: Vec<ComponentId> = routed.into_iter().map(|(profile, _)| profile.id).collect();
    if ids.is_empty() {
        RouteDecision::Unresolved {
            reason: format!("signal {} 没有允许的候选组件", signal_kind_name(signal)),
        }
    } else if ids.len() == 1 {
        let reason = if tool == "Agent" {
            "Agent 配置/派发问题".to_string()
        } else {
            format!("内置工具 {tool} 的执行纪律问题")
        };
        RouteDecision::Exact {
            component: ids[0].clone(),
            reason,
        }
    } else {
        RouteDecision::Choose {
            candidates: ids,
            reason: format!("内置工具 {tool} 的执行纪律问题有多个职责面"),
        }
    }
}

fn signal_kind_name(signal: &EvidenceSignal) -> &str {
    signal.kind.as_str()
}

/// 校验单条 signal 是否允许落到目标组件。
pub fn signal_allows_component(
    signal: &EvidenceSignal,
    component: &ComponentId,
    context: &RouteContext,
) -> bool {
    let profile = profile_for(component);
    profile_allows(&profile, signal, context)
}

/// single-phase 止血门禁：每个 edit MUST 位于当前组件 chunk，且至少一个
/// 本块 signal 允许路由到该 edit 目标；delete 不允许。
pub fn validate_change_set_routes(
    change_set: &ChangeSet,
    evidence: &EvidenceBundle,
    chunk: &[(ComponentId, crate::domain::component::ComponentContent)],
    context: &RouteContext,
) -> Result<(), crate::domain::errors::HarnessError> {
    for edit in &change_set.edits {
        let id = match edit {
            crate::domain::changeset::ComponentEdit::Upsert { id, .. } => id,
            crate::domain::changeset::ComponentEdit::Delete { id } => {
                return Err(crate::domain::errors::HarnessError::invalid(
                    crate::constants::ERROR_DOMAIN,
                    format!("路由门禁拒绝删除组件 {id}"),
                ));
            }
        };
        if !chunk.iter().any(|(candidate, _)| candidate == id) {
            return Err(crate::domain::errors::HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                format!("路由门禁拒绝跨组件块修改 {id}"),
            ));
        }
        let allowed = evidence
            .signals
            .iter()
            .any(|signal| signal_allows_component(signal, id, context));
        if !allowed {
            return Err(crate::domain::errors::HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                format!("signal 不允许路由到组件 {id}"),
            ));
        }
    }
    Ok(())
}
