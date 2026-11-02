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
    pub step_id: String,
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

/// 可回读 session dataset 的最小来源标识。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSourceRef {
    pub session_id: String,
    pub run_id: String,
    pub step_id: String,
}

impl EvidenceSourceRef {
    fn from_event(event: &SessionEventRecord) -> Option<Self> {
        if event.session_id.is_empty()
            || event.session_id == "unknown"
            || event.run_id.is_empty()
            || event.run_id == "unknown"
            || event.step_id.is_empty()
            || event.step_id == "unknown"
        {
            return None;
        }
        Some(Self {
            session_id: event.session_id.clone(),
            run_id: event.run_id.clone(),
            step_id: event.step_id.clone(),
        })
    }
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
    /// 可回读 session 的有界来源（旧序列化数据缺字段时为空）。
    #[serde(default)]
    pub sources: Vec<EvidenceSourceRef>,
}

fn default_occurrences() -> usize {
    1
}

/// 从 session dataset 回读的有界上下文（只进 RSI prompt，不落 canonical）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionContextExcerpt {
    pub source: EvidenceSourceRef,
    pub text: String,
}

/// 按字符数截断；发生截断时省略号计入预算，输出 NEVER 超过 `bound`。
pub(crate) fn truncate_with_marker(text: &str, bound: usize) -> String {
    if text.chars().count() <= bound {
        return text.to_string();
    }
    if bound == 0 {
        return String::new();
    }
    let mut output: String = text.chars().take(bound - 1).collect();
    output.push('…');
    output
}

/// 一个证据块（一段 session/run 的聚合结果，#2049 流模型的分块单位）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    /// 按优先级排序：repeated_failure > retry_pattern > oversized_result > run_outcome。
    pub signals: Vec<EvidenceSignal>,
    /// 覆盖的 session 数（诊断用）。
    pub sessions_covered: usize,
}

/// 单类信号聚合中的可变事实。
#[derive(Default)]
struct SignalAccumulator {
    count: usize,
    tool: String,
    excerpt: String,
    fingerprint: String,
    sources: Vec<EvidenceSourceRef>,
}

impl SignalAccumulator {
    fn observe(&mut self, event: &SessionEventRecord) {
        self.count += 1;
        if let Some(tool) = &event.tool_name {
            self.tool = tool.clone();
        }
        if let Some(error_text) = &event.error_text {
            self.excerpt = error_text.clone();
        }
        if let Some(fingerprint) = &event.call_fingerprint {
            self.fingerprint = fingerprint.clone();
        }
        if let Some(source) = EvidenceSourceRef::from_event(event) {
            if self.sources.len() < crate::constants::EVIDENCE_SOURCE_REF_LIMIT
                && !self.sources.contains(&source)
            {
                self.sources.push(source);
            }
        }
    }
}

/// 从事件流聚合一个证据块（纯函数）。
pub fn aggregate_evidence(events: &[SessionEventRecord]) -> EvidenceBundle {
    let mut signals = Vec::new();
    let mut failure_counts: HashMap<(String, String), SignalAccumulator> = HashMap::new();
    let mut oversized_by_tool: HashMap<String, SignalAccumulator> = HashMap::new();
    let mut fingerprint_counts: HashMap<String, SignalAccumulator> = HashMap::new();
    let mut run_outcomes: HashMap<String, SignalAccumulator> = HashMap::new();
    let mut sessions: HashMap<String, usize> = HashMap::new();

    for event in events {
        sessions.insert(event.session_id.clone(), 1);
        if event.is_error {
            if let (Some(tool), Some(error_text)) = (&event.tool_name, &event.error_text) {
                let signature = error_signature(error_text);
                failure_counts
                    .entry((tool.clone(), signature))
                    .or_default()
                    .observe(event);
            }
        }
        if let Some(tool) = &event.tool_name {
            if event.result_chars >= OVERSIZED_RESULT_CHARS {
                oversized_by_tool
                    .entry(tool.clone())
                    .or_default()
                    .observe(event);
            }
            if let Some(fingerprint) = &event.call_fingerprint {
                fingerprint_counts
                    .entry(fingerprint.clone())
                    .or_default()
                    .observe(event);
            }
        } else {
            // run 级事件：error_text 承载终态（adapter 约定）
            let outcome = event.error_text.clone().unwrap_or_else(|| "unknown".into());
            run_outcomes.entry(outcome).or_default().observe(event);
        }
    }

    let mut repeated: Vec<_> = failure_counts
        .into_iter()
        .filter(|(_, entry)| entry.count >= 2)
        .map(|((tool, signature), entry)| EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: format!("repeated_failure:{tool}:{signature}"),
            summary: format!("工具 {tool} 重复失败 ×{}（签名：{signature}）", entry.count),
            excerpt: truncate_chars(&entry.excerpt, EVIDENCE_EXCERPT_BOUND),
            occurrences: entry.count,
            sources: entry.sources,
        })
        .collect();
    repeated.sort_by(|a, b| b.summary.cmp(&a.summary));
    signals.extend(repeated);

    let mut retries: Vec<_> = fingerprint_counts
        .into_iter()
        .filter(|(_, entry)| entry.count >= 2)
        .map(|(fingerprint, entry)| EvidenceSignal {
            kind: SignalKind::RetryPattern,
            signal_id: format!("retry_pattern:{}:{fingerprint}", entry.tool),
            summary: format!(
                "工具 {} 同参数重复调用 ×{}（疑似重试）",
                entry.tool, entry.count
            ),
            excerpt: None,
            occurrences: entry.count,
            sources: entry.sources,
        })
        .collect();
    retries.sort_by(|a, b| b.summary.cmp(&a.summary));
    signals.extend(retries);

    let mut oversized: Vec<_> = oversized_by_tool
        .into_iter()
        .map(|(tool, entry)| EvidenceSignal {
            kind: SignalKind::OversizedResult,
            signal_id: format!("oversized_result:{tool}"),
            summary: format!(
                "工具 {tool} 超长结果（≥{OVERSIZED_RESULT_CHARS} 字符）×{}",
                entry.count
            ),
            excerpt: None,
            occurrences: entry.count,
            sources: entry.sources,
        })
        .collect();
    oversized.sort_by(|a, b| b.summary.cmp(&a.summary));
    signals.extend(oversized);

    if !run_outcomes.is_empty() {
        let mut entries: Vec<_> = run_outcomes.into_iter().collect();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let parts: Vec<String> = entries
            .iter()
            .map(|(outcome, entry)| format!("{outcome}×{}", entry.count))
            .collect();
        let occurrences: usize = entries.iter().map(|(_, entry)| entry.count).sum();
        let sources = entries
            .into_iter()
            .flat_map(|(_, entry)| entry.sources)
            .take(crate::constants::EVIDENCE_SOURCE_REF_LIMIT)
            .collect::<Vec<_>>();
        signals.push(EvidenceSignal {
            kind: SignalKind::RunOutcome,
            signal_id: "run_outcome:aggregate".to_string(),
            summary: format!("run 终态分布：{}", parts.join(", ")),
            excerpt: None,
            occurrences,
            sources,
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

pub(crate) fn truncate_chars(text: &str, bound: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= bound {
        return Some(text.to_string());
    }
    let mut truncated: String = text.chars().take(bound.saturating_sub(1)).collect();
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
