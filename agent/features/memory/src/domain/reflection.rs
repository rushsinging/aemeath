use super::{MemoryCategory, MemoryEntry, MemoryError, MemoryId, MemoryLayer};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

fn null_as_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn default_memory_layer() -> MemoryLayer {
    MemoryLayer::Project
}

/// 观测/日志用的引用 token 截断：模型编造的引用可能是很长的内容片段，
/// 日志只保留可辨识前缀，避免污染日志与泄露正文。
pub(crate) fn truncate_reference_token(token: &str, max_chars: usize) -> String {
    if token.chars().count() <= max_chars {
        return token.to_string();
    }
    let mut truncated: String = token.chars().take(max_chars).collect();
    truncated.push('…');
    truncated
}

/// A candidate memory produced by Reflection, before it becomes a `MemoryEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySuggestion {
    #[serde(default = "default_memory_layer")]
    pub layer: MemoryLayer,
    pub category: MemoryCategory,
    pub content: String,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub reason: String,
    /// 本建议取代哪些已有记忆（apply 时建立 `superseded_by = 留存条目 id`）。
    /// 只由 apply 消费，不直接写库（#1774）。
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub supersedes: Vec<MemoryId>,
    /// 本建议归纳自哪些已有记忆（apply 后成为新条目的 `evidence`，且置
    /// `kind = Synthesized`）。少于两条来源不是归纳而是复制，M13 要求
    /// 降级为普通建议（#1776）。
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub synthesizes: Vec<MemoryId>,
}

/// The complete published-language response expected from a Reflection model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionOutput {
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub deviations: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub suggested_memories: Vec<MemorySuggestion>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub outdated_memories: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ReflectionError {
    #[error("reflection response JSON is invalid")]
    Parse,
    #[error("reflection response could not be parsed as JSON")]
    Unparseable,
    #[error("invalid reflection memory suggestion: {0}")]
    InvalidSuggestion(String),
    #[error(transparent)]
    Memory(#[from] MemoryError),
}

pub type ReflectionResult<T> = Result<T, ReflectionError>;

/// 反思输入引用表：本次运行内稳定的行序号（从 1 起）→ 已有记忆 id。
///
/// 反思模型只看得到行首序号（`[M1]`、`[M2]`…），NEVER 看到 UUID；解析模型输出
/// 时经本表把序号映射回真实 id。序号仅在产生它的那次反思运行内有效。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflectionReferenceTable {
    references: Vec<(u32, MemoryId)>,
}

impl ReflectionReferenceTable {
    /// 行序 MUST 与 [`ReflectionEngine::format_memory_summary`] 的渲染顺序一致
    /// （两者都以同一 `entries` 切片为基准）。
    pub fn from_entries(entries: &[MemoryEntry]) -> Self {
        Self {
            references: entries
                .iter()
                .enumerate()
                .map(|(index, entry)| (index as u32 + 1, entry.id))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.references.len()
    }

    pub fn is_empty(&self) -> bool {
        self.references.is_empty()
    }

    /// 解析一条模型引用。接受两类形式：
    /// - 完整 UUID（兼容模型直填真实 id 的情况）；
    /// - 本表行序号：`M3` / `m3` / `[M3]` / `#3` / `3`。
    ///
    /// 无法解析时返回 `None`，由调用方降级（跳过并记录），NEVER 失败整批。
    pub fn resolve(&self, token: &str) -> Option<MemoryId> {
        let trimmed = token.trim();
        if let Ok(id) = MemoryId::new(trimmed) {
            return Some(id);
        }
        let ordinal = parse_reference_ordinal(trimmed)?;
        self.references
            .iter()
            .find(|(candidate, _)| *candidate == ordinal)
            .map(|(_, id)| *id)
    }
}

fn parse_reference_ordinal(token: &str) -> Option<u32> {
    let stripped = token.trim();
    let stripped = stripped.strip_prefix('[').unwrap_or(stripped);
    let stripped = stripped.strip_suffix(']').unwrap_or(stripped);
    let stripped = stripped.trim();
    let stripped = stripped
        .strip_prefix('M')
        .or_else(|| stripped.strip_prefix('m'))
        .or_else(|| stripped.strip_prefix('#'))
        .unwrap_or(stripped);
    stripped
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|ordinal| *ordinal > 0)
}

/// 逐条解析建议里的引用 token：可解析者映射为真实 id，无法解析者跳过并记录
/// （NEVER 失败整批）。
fn resolve_reference_tokens(
    tokens: Vec<String>,
    references: &ReflectionReferenceTable,
    field: ReflectionReferenceField,
    unresolved: &mut Vec<UnresolvedReflectionReference>,
) -> Vec<MemoryId> {
    tokens
        .into_iter()
        .filter_map(|token| match references.resolve(&token) {
            Some(id) => Some(id),
            None => {
                unresolved.push(UnresolvedReflectionReference {
                    field,
                    token: truncate_reference_token(&token, 80),
                });
                None
            }
        })
        .collect()
}

/// 反思输入构造产物：prompt 文本 + 引用表（解析模型输出时使用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionPrompt {
    pub text: String,
    pub references: ReflectionReferenceTable,
}

/// 无法解析的引用所属字段（观测分类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionReferenceField {
    Outdated,
    Supersedes,
    Synthesizes,
}

/// 一条无法解析的模型引用：原始 token 截断后保留，供日志观测。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedReflectionReference {
    pub field: ReflectionReferenceField,
    pub token: String,
}

/// 解析后的反思输出：引用（序号/UUID）已映射为真实 id；`unresolved` 记录被
/// 跳过的引用（NEVER 因单条坏引用使整批失败，也 NEVER 丢弃合法建议）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedReflectionOutput {
    pub output: ReflectionOutput,
    pub unresolved: Vec<UnresolvedReflectionReference>,
}

/// 模型输出的 wire 投影：引用字段以字符串承载（序号、UUID 或任意编造串），
/// 经引用表解析后才进入领域类型 [`ReflectionOutput`]。
#[derive(Debug, Clone, Deserialize)]
struct RawReflectionOutput {
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    deviations: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    suggested_memories: Vec<RawMemorySuggestion>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    outdated_memories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawMemorySuggestion {
    #[serde(default = "default_memory_layer")]
    layer: MemoryLayer,
    category: MemoryCategory,
    content: String,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    tags: Vec<String>,
    #[serde(default)]
    reason: String,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    supersedes: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    synthesizes: Vec<String>,
}

/// A provider-independent message projection used by the pure Reflection service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionMessage {
    pub role: String,
    pub text: String,
}

impl ReflectionMessage {
    pub fn new(role: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionTokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionTrigger {
    Interval,
    PreCompact,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionErrorCategory {
    LlmCall,
    EmptyResponse,
    Parse,
    InvalidSuggestion,
    Apply,
    History,
    Cancelled,
    TimedOut,
    /// 进程被终止（崩溃/重启）导致 Running 事实未收口，由悬挂收口（reap）写入。
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionApplyStatus {
    NotApplied,
    Applied,
    PartiallyApplied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionSafeSummary {
    pub id: String,
    pub timestamp: u64,
    pub trigger: ReflectionTrigger,
    pub status: ReflectionStatus,
    pub deviations: usize,
    pub suggestions: usize,
    pub outdated: usize,
    pub apply_status: ReflectionApplyStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_category: Option<ReflectionErrorCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<ReflectionTokenUsage>,
    pub duration_ms: u64,
    /// 反思游标（元数据非内容，Safe 边界允许携带）：见 `ReflectionRecord::coverage_end`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_end: Option<u64>,
    /// 偏差观察文本：仅 `safe_summary_with_content()` 显式投影时携带（本地
    /// /reflect 查询）；默认 `safe_summary()` 为 None（Safe 边界不携带内容）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deviation_texts: Option<Vec<String>>,
    /// 建议记忆内容：同 `deviation_texts` 的显式投影口径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_memories: Option<Vec<MemorySuggestion>>,
}

/// 申请结果值对象：Reflection 建议的应用完成度（原摆放于 ports，依赖方向回归 domain）。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ReflectionApplyResult {
    /// Number of requested operations (suggestions plus outdated-memory marks).
    pub attempted: usize,
    /// Number of operations durably completed. This can be smaller than
    /// `attempted` when a cross-layer apply returns `MemoryError::PartialApply`.
    pub completed: usize,
    pub suggestions_added: usize,
    pub outdated_marked: usize,
    /// Supersede relations durably established (#1774). A relation rejected by
    /// the cycle guard (M9) counts in neither this nor `completed`, so
    /// `attempted - completed` stays the honest skipped-operation count.
    pub superseded: usize,
}

/// One completed Reflection result. Persistence is supplied by a separate adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionRecord {
    pub id: String,
    pub timestamp: u64,
    pub trigger: ReflectionTrigger,
    pub status: ReflectionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<ReflectionOutput>,
    pub apply_result: Option<ReflectionApplyResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_category: Option<ReflectionErrorCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<ReflectionTokenUsage>,
    pub duration_ms: u64,
    /// 反思游标：本次反思覆盖到的 session active 历史终点（structured_messages 的
    /// 消息计数）。仅 Succeeded 终态推进；缺失（旧记录/未推进）时调用方回退。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_end: Option<u64>,
}

impl ReflectionRecord {
    pub fn running(id: impl Into<String>, timestamp: u64, trigger: ReflectionTrigger) -> Self {
        Self {
            id: id.into(),
            timestamp,
            trigger,
            status: ReflectionStatus::Running,
            output: None,
            apply_result: None,
            error_category: None,
            token_usage: None,
            duration_ms: 0,
            coverage_end: None,
        }
    }

    pub fn failed(
        id: impl Into<String>,
        timestamp: u64,
        trigger: ReflectionTrigger,
        error_category: ReflectionErrorCategory,
        duration_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            timestamp,
            trigger,
            status: ReflectionStatus::Failed,
            output: None,
            apply_result: None,
            error_category: Some(error_category),
            token_usage: None,
            duration_ms,
            coverage_end: None,
        }
    }

    pub fn safe_summary(&self) -> ReflectionSafeSummary {
        let (deviations, suggestions, outdated) = self
            .output
            .as_ref()
            .map(|output| {
                (
                    output.deviations.len(),
                    output.suggested_memories.len(),
                    output.outdated_memories.len(),
                )
            })
            .unwrap_or_default();
        let apply_status = match &self.apply_result {
            None => ReflectionApplyStatus::NotApplied,
            Some(result) if result.completed < result.attempted => {
                ReflectionApplyStatus::PartiallyApplied
            }
            Some(_) => ReflectionApplyStatus::Applied,
        };
        ReflectionSafeSummary {
            id: self.id.clone(),
            timestamp: self.timestamp,
            trigger: self.trigger,
            status: self.status,
            deviations,
            suggestions,
            outdated,
            apply_status,
            error_category: self.error_category,
            token_usage: self.token_usage,
            duration_ms: self.duration_ms,
            coverage_end: self.coverage_end,
            deviation_texts: None,
            suggested_memories: None,
        }
    }

    /// 显式内容投影：在 `safe_summary()` 基础上携带偏差文本与建议内容。
    /// 仅供本地 /reflect 历史查询使用——Safe 边界（默认摘要不携带内容）不变。
    pub fn safe_summary_with_content(&self) -> ReflectionSafeSummary {
        let mut summary = self.safe_summary();
        if let Some(output) = &self.output {
            summary.deviation_texts = Some(output.deviations.clone());
            summary.suggested_memories = Some(output.suggested_memories.clone());
        }
        summary
    }
}

/// Stateless implementation of the Memory Reflection domain service.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReflectionEngine;

impl ReflectionEngine {
    fn extract_json_object(text: &str) -> Option<&str> {
        let start = text.find('{')?;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;

        for (offset, ch) in text[start..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' if in_string => escaped = true,
                '"' => in_string = !in_string,
                '{' if !in_string => depth += 1,
                '}' if !in_string => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        // 偏移累计自 char_indices 的 len_utf8()，边界由遍历构造保证。
                        return Some(&text[start..start + offset + ch.len_utf8()]);
                        // allow unsafe_text_op
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn prompt_template(lang: &str) -> &'static str {
        if lang == "zh" {
            r#"你是 Aemeath 的 Reflection 引擎。请根据当前项目记忆和最近对话摘要，检查行为偏差、提出应写入的长期记忆、识别过时记忆。

要求：
- 只输出 JSON，不要输出 Markdown。
- suggested_memories[].layer 只能是 project 或 global，默认优先使用 project。
- suggested_memories[].category 只能是 fact、decision、preference、pattern、pitfall。
- 引用已有记忆只能使用「当前项目记忆」列表行首的序号（形如 M3，对应行首 [M3]）。
  NEVER 编造列表中不存在的序号，NEVER 用标签、正文或任何其他标识充当引用。
  正确示例："outdated_memories": ["M2"]。
  错误示例："outdated_memories": ["[Decision][some-tag]"] 或 ["some-tag-slug"]。
- outdated_memories 填被新事实推翻、不应再注入的条目序号。
- suggested_memories[].synthesizes 只在**归纳多条已有记忆**时填这些条目序号。
  单条来源不是归纳（那只是改写），此时留空数组——不足两条来源的归纳 MUST NOT
  产出。结论被新证据修正时在正文里写「曾…现…」，不要输出置信度数字。
- suggested_memories[].supersedes 只在**新记忆明确取代某条已有记忆**时填该
  条目序号（例如部署方式、端口、结论被新事实推翻）。两条记忆只是补充关系、或
  旧记忆仍然成立时，**必须留空数组**——误取代会让仍有价值的记忆停止注入。
- 没有内容时输出空数组。

JSON 格式：
{{
    "deviations": ["偏差描述"],
    "suggested_memories": [{{"layer":"project","category":"decision","content":"记忆内容","tags":["可选标签"],"reason":"为什么建议添加","supersedes":[],"synthesizes":[]}}],
    "outdated_memories": ["M3"]
}}

# 当前项目记忆
{project_memory}

# 最近对话摘要
{recent_summary}"#
        } else {
            r#"You are the Aemeath Reflection engine. Based on the current project memory and recent conversation summary, detect behavioral deviations, suggest long-term memories to write, and identify outdated memories.

Requirements:
- Output JSON only, no Markdown.
- suggested_memories[].layer must be project or global; prefer project by default.
- suggested_memories[].category must be fact, decision, preference, pattern, or pitfall.
- Reference existing memories ONLY by the ordinal shown at the start of each
  "Current project memory" line (e.g. M3 for a line beginning with [M3]).
  NEVER invent an ordinal that is not in the list; NEVER use tags, content, or
  any other identifier as a reference.
  Good: "outdated_memories": ["M2"].
  Bad: "outdated_memories": ["[Decision][some-tag]"] or ["some-tag-slug"].
- outdated_memories lists the ordinals of entries superseded by new facts and
  that must stop being injected.
- Fill suggested_memories[].synthesizes with the ordinals ONLY when this
  suggestion combines several existing memories into a new conclusion. A single
  source is not a synthesis — it is a restatement — so leave it empty; a
  synthesis over fewer than two sources must not be produced. When new evidence
  corrects a conclusion, phrase the change in the content ("used to …, now …")
  instead of emitting a confidence number.
- Fill suggested_memories[].supersedes with an ordinal ONLY when the
  new memory explicitly replaces it (a changed deploy target, port, or reversed
  conclusion). Leave it empty when the two memories merely complement each
  other or the old one still holds — a wrong supersede stops a still-valuable
  memory from being injected.
- Output empty arrays when there is nothing.

JSON format:
{{
    "deviations": ["deviation description"],
    "suggested_memories": [{{"layer":"project","category":"decision","content":"memory content","tags":["optional tag"],"reason":"why this is suggested","supersedes":[],"synthesizes":[]}}],
    "outdated_memories": ["M3"]
}}

# Current project memory
{project_memory}

# Recent conversation summary
{recent_summary}"#
        }
    }
}

impl ReflectionEngine {
    pub fn build_prompt(&self, project_memory: &str, recent_summary: &str, lang: &str) -> String {
        Self::prompt_template(lang)
            .replace("{project_memory}", project_memory)
            .replace("{recent_summary}", recent_summary)
    }

    /// 解析模型输出并把引用（序号/UUID）映射为真实 id。
    ///
    /// 单条无法解析的引用被跳过并记录到 `unresolved`——NEVER 使整批失败，
    /// NEVER 丢弃合法建议（引用契约事故形态的根因修复）。
    pub fn parse_output(
        &self,
        raw: &str,
        references: &ReflectionReferenceTable,
    ) -> ReflectionResult<ResolvedReflectionOutput> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ReflectionError::Unparseable);
        }

        let fenced = trimmed
            .split_once("```json")
            .and_then(|(_, rest)| rest.split_once("```").map(|(json, _)| json.trim()));
        let source = if let Some(json) = fenced {
            json
        } else if let Some(json) = Self::extract_json_object(trimmed) {
            json
        } else if trimmed.starts_with('{') {
            // Preserve serde's structural error for JSON-looking, incomplete input.
            trimmed
        } else {
            return Err(ReflectionError::Unparseable);
        };

        let raw_output: RawReflectionOutput =
            serde_json::from_str(source).map_err(|_| ReflectionError::Parse)?;
        let mut unresolved = Vec::new();
        let mut suggested_memories = Vec::with_capacity(raw_output.suggested_memories.len());
        for (index, suggestion) in raw_output.suggested_memories.into_iter().enumerate() {
            if suggestion.content.trim().is_empty() {
                return Err(ReflectionError::InvalidSuggestion(format!(
                    "suggested_memories[{index}].content must not be empty"
                )));
            }
            suggested_memories.push(MemorySuggestion {
                layer: suggestion.layer,
                category: suggestion.category,
                content: suggestion.content,
                tags: suggestion.tags,
                reason: suggestion.reason,
                supersedes: resolve_reference_tokens(
                    suggestion.supersedes,
                    references,
                    ReflectionReferenceField::Supersedes,
                    &mut unresolved,
                ),
                synthesizes: resolve_reference_tokens(
                    suggestion.synthesizes,
                    references,
                    ReflectionReferenceField::Synthesizes,
                    &mut unresolved,
                ),
            });
        }
        let outdated_memories = raw_output
            .outdated_memories
            .into_iter()
            .filter_map(|token| match references.resolve(&token) {
                Some(id) => Some(id.to_string()),
                None => {
                    unresolved.push(UnresolvedReflectionReference {
                        field: ReflectionReferenceField::Outdated,
                        token: truncate_reference_token(&token, 80),
                    });
                    None
                }
            })
            .collect();

        Ok(ResolvedReflectionOutput {
            output: ReflectionOutput {
                deviations: raw_output.deviations,
                suggested_memories,
                outdated_memories,
            },
            unresolved,
        })
    }

    /// 渲染反思输入的记忆列表：每行以本次运行内稳定的行序号开头
    /// （`- [M1] [Decision][tag] content`），同时返回序号表。模型 NEVER 看到
    /// UUID；解析模型输出时经 [`ReflectionReferenceTable`] 映射回真实 id。
    ///
    /// 行文本与引用表由同一 `entries` 切片推导（序号单一来源），顺序 MUST 一致。
    pub fn format_memory_summary(
        &self,
        entries: &[MemoryEntry],
    ) -> (String, ReflectionReferenceTable) {
        let references = ReflectionReferenceTable::from_entries(entries);
        let text = references
            .references
            .iter()
            .zip(entries.iter())
            .map(|((ordinal, _), entry)| {
                format!(
                    "- [M{}] [{:?}][{}] {}",
                    ordinal,
                    entry.category,
                    entry.tags.join(","),
                    entry.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        (text, references)
    }

    pub fn recent_messages_summary(
        &self,
        messages: &[ReflectionMessage],
        max_chars: usize,
    ) -> String {
        if max_chars == 0 {
            return String::new();
        }

        let mut recent = Vec::new();
        for message in messages.iter().rev() {
            if message.text.trim().is_empty() {
                continue;
            }
            let role = if message.role.eq_ignore_ascii_case("user") {
                "User"
            } else if message.role.eq_ignore_ascii_case("assistant") {
                "Assistant"
            } else {
                message.role.as_str()
            };
            recent.push(format!("[{role}]: {}", message.text));
            let summary = recent.iter().rev().cloned().collect::<Vec<_>>().join("\n");
            if summary.chars().count() >= max_chars {
                return summary.chars().take(max_chars).collect();
            }
        }
        recent.into_iter().rev().collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
#[path = "reflection_tests.rs"]
mod tests;
