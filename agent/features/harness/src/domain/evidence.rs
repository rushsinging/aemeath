//! 执行证据 domain（#2048，设计 04 分片 §2A）。
//!
//! 信号分两级：`SessionEventRecord` 是 dataset 读取 adapter 产出的
//! 扁平事件流（IO 与逻辑分离）；`EvidenceBundle` 是聚合后的材料信号，
//! 按 Harness² 三级优先排序注入改进 prompt。
//!
//! v1 交付四类可从既有落盘稳定提取的信号：repeated_failure /
//! retry_pattern / oversized_result / run_outcome；user_correction 与
//! token_efficiency 待交互事件与 usage 汇总投影补齐（登记于 #2049 后续）。

use crate::constants::{ERROR_DOMAIN, EVIDENCE_EXCERPT_BOUND, OVERSIZED_RESULT_CHARS};
use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 信号种类（lesson 证据绑定的 signal 枚举，04 分片 §2A.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// v1 遗留：模型自述提案理由（降为兜底信号）。
    CycleObservation,
    /// 同 (tool, 错误签名) 重复失败。
    RepeatedFailure,
    /// 同工具近似参数重复调用（Edit old_string 不匹配重试等）。
    RetryPattern,
    /// 触发落盘阈值的超长工具结果。
    OversizedResult,
    /// run 终态分布。
    RunOutcome,
}

impl SignalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CycleObservation => "cycle_observation",
            Self::RepeatedFailure => "repeated_failure",
            Self::RetryPattern => "retry_pattern",
            Self::OversizedResult => "oversized_result",
            Self::RunOutcome => "run_outcome",
        }
    }

    /// 渲染/裁剪优先级（小者优先；对齐 §2A.2 聚合优先级）。
    pub fn render_priority(self) -> u8 {
        match self {
            Self::RepeatedFailure => 0,
            Self::RetryPattern => 1,
            Self::OversizedResult => 2,
            Self::RunOutcome => 3,
            Self::CycleObservation => 4,
        }
    }
}

/// dataset 扫描产出的单条扁平事件（adapter → 聚合器的输入）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEventRecord {
    pub session_id: String,
    pub run_id: String,
    /// 工具名；None 表示 run 级事件（终态等）。
    pub tool_name: Option<String>,
    pub is_error: bool,
    /// 错误文本（is_error 时；截断至摘录上限由 adapter 负责）。
    pub error_text: Option<String>,
    /// 工具结果字符数。
    pub result_chars: usize,
    /// 调用指纹（tool + 关键参数摘要），retry 识别用。
    pub call_fingerprint: Option<String>,
}

/// 聚合后的单条证据信号。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSignal {
    pub kind: SignalKind,
    pub signal_id: String,
    /// 人读描述（含计数与工具名，进 prompt 的 evidence 块）。
    pub summary: String,
    /// 代表性摘录（有界）。
    pub excerpt: Option<String>,
    /// 本次聚合内的出现次数（#2092 强度门禁的结构化输入；
    /// 旧数据缺该字段时按 1 计）。
    #[serde(default = "default_occurrences")]
    pub occurrences: usize,
}

fn default_occurrences() -> usize {
    1
}

/// 一个证据块（一段 session/run 的聚合结果，#2049 流模型的分块单位）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    /// 按优先级排序：repeated_failure > retry_pattern > oversized_result > run_outcome。
    pub signals: Vec<EvidenceSignal>,
    /// 覆盖的 session 数（诊断用）。
    pub sessions_covered: usize,
}

/// 从事件流聚合一个证据块（纯函数）。
pub fn aggregate_evidence(events: &[SessionEventRecord]) -> EvidenceBundle {
    let mut signals = Vec::new();

    // repeated_failure：同 (tool, 错误签名) 计数 ≥2
    let mut failure_counts: HashMap<(String, String), (usize, String, String)> = HashMap::new();
    let mut oversized_by_tool: HashMap<String, usize> = HashMap::new();
    let mut fingerprint_counts: HashMap<String, (usize, String)> = HashMap::new();
    let mut run_outcomes: HashMap<String, usize> = HashMap::new();
    let mut sessions: HashMap<String, usize> = HashMap::new();

    for event in events {
        sessions.insert(event.session_id.clone(), 1);
        if event.is_error {
            if let (Some(tool), Some(error_text)) = (&event.tool_name, &event.error_text) {
                let signature = error_signature(error_text);
                let entry = failure_counts
                    .entry((tool.clone(), signature.clone()))
                    .or_insert((0, tool.clone(), error_text.clone()));
                entry.0 += 1;
            }
        }
        if let Some(tool) = &event.tool_name {
            if event.result_chars >= OVERSIZED_RESULT_CHARS {
                *oversized_by_tool.entry(tool.clone()).or_insert(0) += 1;
            }
            if let Some(fingerprint) = &event.call_fingerprint {
                let entry = fingerprint_counts
                    .entry(fingerprint.clone())
                    .or_insert((0, tool.clone()));
                entry.0 += 1;
            }
        } else {
            // run 级事件：error_text 承载终态（adapter 约定）
            let outcome = event.error_text.clone().unwrap_or_else(|| "unknown".into());
            *run_outcomes.entry(outcome).or_insert(0) += 1;
        }
    }

    let mut repeated: Vec<_> = failure_counts
        .into_iter()
        .filter(|(_, (count, _, _))| *count >= 2)
        .map(|((tool, signature), (count, _, excerpt))| EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: format!("repeated_failure:{tool}:{signature}"),
            summary: format!("工具 {tool} 重复失败 ×{count}（签名：{signature}）"),
            excerpt: truncate_chars(&excerpt, EVIDENCE_EXCERPT_BOUND),
            occurrences: count,
        })
        .collect();
    repeated.sort_by(|a, b| b.summary.cmp(&a.summary));
    signals.extend(repeated);

    let mut retries: Vec<_> = fingerprint_counts
        .into_iter()
        .filter(|(_, (count, _))| *count >= 2)
        .map(|(fingerprint, (count, tool))| EvidenceSignal {
            kind: SignalKind::RetryPattern,
            signal_id: format!("retry_pattern:{tool}:{fingerprint}"),
            summary: format!("工具 {tool} 同参数重复调用 ×{count}（疑似重试）"),
            excerpt: None,
            occurrences: count,
        })
        .collect();
    retries.sort_by(|a, b| b.summary.cmp(&a.summary));
    signals.extend(retries);

    let mut oversized: Vec<_> = oversized_by_tool
        .into_iter()
        .map(|(tool, count)| EvidenceSignal {
            kind: SignalKind::OversizedResult,
            signal_id: format!("oversized_result:{tool}"),
            summary: format!("工具 {tool} 超长结果（≥{OVERSIZED_RESULT_CHARS} 字符）×{count}"),
            excerpt: None,
            occurrences: count,
        })
        .collect();
    oversized.sort_by(|a, b| b.summary.cmp(&a.summary));
    signals.extend(oversized);

    if !run_outcomes.is_empty() {
        let mut parts: Vec<String> = run_outcomes
            .into_iter()
            .map(|(outcome, count)| format!("{outcome}×{count}"))
            .collect();
        parts.sort();
        signals.push(EvidenceSignal {
            kind: SignalKind::RunOutcome,
            signal_id: "run_outcome:aggregate".to_string(),
            summary: format!("run 终态分布：{}", parts.join(", ")),
            excerpt: None,
            occurrences: 1,
        });
    }

    EvidenceBundle {
        signals,
        sessions_covered: sessions.len(),
    }
}

/// 错误签名：错误文本首行、去数字与引号后的紧凑形态。
fn error_signature(error_text: &str) -> String {
    let first_line = error_text.lines().next().unwrap_or("").trim();
    let mut signature = String::with_capacity(first_line.len());
    for ch in first_line.chars() {
        if ch.is_ascii_digit() || ch == '"' || ch == '\'' {
            signature.push('#');
        } else {
            signature.push(ch);
        }
    }
    let bounded = truncate_chars(signature.trim(), 60).unwrap_or_default();
    bounded
}

fn truncate_chars(text: &str, bound: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= bound {
        return Some(text.to_string());
    }
    let mut truncated: String = text.chars().take(bound).collect();
    truncated.push('…');
    Some(truncated)
}

/// 校验证据块（prompt 注入前的防御性检查：信号数上限）。
pub fn validate_evidence_bundle(bundle: &EvidenceBundle) -> Result<(), HarnessError> {
    if bundle.signals.len() > 64 {
        return Err(HarnessError::invalid(
            ERROR_DOMAIN,
            format!("证据信号数超上限：{} > 64", bundle.signals.len()),
        ));
    }
    Ok(())
}
