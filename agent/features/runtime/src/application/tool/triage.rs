//! 权限/风险预筛（#1836）：System One noul 评分对 policy `Allow` 判定的
//! **单向加严**。
//!
//! 协议约束（调用方 MUST 遵守，coordination 层测试锁定）：
//! - 只在规则引擎给出 `Allow` 之后调用——评分结果只能把 `Allow` 提级为
//!   `RequireApproval`，NEVER 放宽既有 `Deny` / `RequireApproval`；
//! - 评分不可用 / 超时 / 题目构造失败一律静默降级为「不提级」，NEVER
//!   阻断工具执行主路径；
//! - 阈值边界宁严勿松：`p_true == 阈值` 视为高风险（误放率优先）。

use std::sync::Arc;

use crate::constants::{POLICY_TRIAGE_RISK_THRESHOLD, TRIAGE_NOUL_INSTRUCTIONS};
use policy::PolicyRequestData;
use share::tools_vocab::{ToolCapabilities, ToolCapability};

/// 高风险提级结论（含可读中文理由）。
pub struct TriageEscalation {
    /// 提级理由（含工具名与风险概率，供审批交互展示）。
    pub reason: String,
}

/// 权限/风险预筛器：持有场景专属评分端口，对单次工具调用做 noul 风险问询。
pub struct PolicyTriage {
    scoring: Arc<dyn systemone::ScoringPort>,
}

impl PolicyTriage {
    /// 构造（composition 装配期注入 `for_policy_triage` 槽位端口）。
    pub fn new(scoring: Arc<dyn systemone::ScoringPort>) -> Self {
        Self { scoring }
    }

    /// 高风险判定阈值（`p_true >= 阈值` 提级；常量真相见 constants）。
    pub fn risk_threshold() -> f32 {
        POLICY_TRIAGE_RISK_THRESHOLD
    }

    /// 对已获 `Allow` 的工具调用做风险预筛：高风险返回提级结论，低风险或
    /// 评分不可用返回 `None`（放行原判定）。
    pub async fn escalate(&self, request: &PolicyRequestData) -> Option<TriageEscalation> {
        let state = systemone::ScoringState::new(triage_state(request))?;
        let question = systemone::ScoringQuestion::noul(TRIAGE_NOUL_INSTRUCTIONS, None)?;
        let answers = self.scoring.answer(&state, &[question]).await.ok()?;
        let p_true = match answers.first() {
            Some(systemone::ScoringAnswer::Noul { p_true, .. }) => *p_true,
            _ => None?,
        };
        if f64::from(POLICY_TRIAGE_RISK_THRESHOLD) <= p_true {
            Some(TriageEscalation {
                reason: format!(
                    "System One 风险预筛：工具 {} 疑似破坏性/不可逆操作（风险概率 {p_true:.2} ≥ {:.2}），建议人工确认",
                    request.tool_name().as_str(),
                    POLICY_TRIAGE_RISK_THRESHOLD
                ),
            })
        } else {
            None
        }
    }
}

/// 评分 state：工具与能力的事实描述（参数级 state 随真实会话复验迭代）。
fn triage_state(request: &PolicyRequestData) -> String {
    format!(
        "tool call: {} [{}]",
        request.tool_name().as_str(),
        capabilities_text(request.required_capabilities())
    )
}

/// 能力位标志 → 可读文本（按固定位序列出）。
fn capabilities_text(capabilities: ToolCapabilities) -> String {
    const KNOWN: [(ToolCapability, &str); 9] = [
        (ToolCapability::Read, "read"),
        (ToolCapability::Write, "write"),
        (ToolCapability::Execute, "execute"),
        (ToolCapability::NetworkAccess, "network"),
        (ToolCapability::Interact, "interact"),
        (ToolCapability::Dispatch, "dispatch"),
        (ToolCapability::TaskWrite, "task-write"),
        (ToolCapability::WorkspaceControl, "workspace"),
        (ToolCapability::All, "all"),
    ];
    KNOWN
        .iter()
        .filter(|(capability, _)| capabilities.contains_cap(*capability))
        .map(|(_, label)| *label)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
#[path = "triage_tests.rs"]
mod tests;
