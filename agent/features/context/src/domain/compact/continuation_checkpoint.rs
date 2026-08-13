use serde::{Deserialize, Serialize};
use std::fmt;

const SECTION_HEADINGS: [&str; 9] = [
    "Immutable Constraints",
    "Current Objective",
    "Committed Facts",
    "Uncommitted Working Set",
    "Open Decisions / Risks",
    "Resume Cursor",
    "Required Revalidation",
    "Archived Milestones",
    "Continuation Status",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationStatus {
    Continue,
    WaitingForUser,
    Completed,
}

impl ContinuationStatus {
    fn parse(line: &str) -> Result<Self, CheckpointError> {
        let trimmed = line.trim();
        if trimmed == "Continue" || trimmed.starts_with("Continue —") {
            return Ok(Self::Continue);
        }
        if trimmed == "Waiting for User" || trimmed.starts_with("Waiting for User —") {
            return Ok(Self::WaitingForUser);
        }
        if trimmed == "Completed" || trimmed.starts_with("Completed —") {
            return Ok(Self::Completed);
        }
        Err(CheckpointError::InvalidStatus {
            value: line.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeCursor {
    next_action: String,
}

impl ResumeCursor {
    pub fn next_action(&self) -> &str {
        &self.next_action
    }

    pub fn next_action_count(&self) -> usize {
        usize::from(!self.next_action.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeCursorWire {
    pub context: Vec<String>,
    pub next_action: String,
    pub prohibited_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointCompressionPatch {
    pub committed_facts: Vec<String>,
    pub uncommitted_working_set: Vec<String>,
    pub open_decisions_and_risks: Vec<String>,
    pub resume_context: Vec<String>,
    pub required_revalidation: Vec<String>,
    pub archived_milestones: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuationCheckpointWire {
    pub immutable_constraints: Vec<String>,
    pub current_objective: String,
    pub committed_facts: Vec<String>,
    pub uncommitted_working_set: Vec<String>,
    pub open_decisions_and_risks: Vec<String>,
    pub resume_cursor: ResumeCursorWire,
    pub required_revalidation: Vec<String>,
    pub archived_milestones: Vec<String>,
    pub continuation_status: ContinuationStatus,
    pub continuation_reason: String,
}

impl TryFrom<ContinuationCheckpointWire> for ContinuationCheckpoint {
    type Error = CheckpointError;

    fn try_from(wire: ContinuationCheckpointWire) -> Result<Self, Self::Error> {
        let current_objective = normalize_control_text(&wire.current_objective);
        if current_objective.is_empty() {
            return Err(CheckpointError::MissingCurrentObjective);
        }
        let mut resume_cursor_lines = wire
            .resume_cursor
            .context
            .into_iter()
            .map(|line| as_bullet(&line))
            .collect::<Vec<_>>();
        resume_cursor_lines.extend(
            wire.resume_cursor
                .prohibited_actions
                .into_iter()
                .map(|line| format!("- Prohibited: {}", normalize_control_text(&line))),
        );
        Self::from_sections(CheckpointSections {
            immutable_constraints: wire
                .immutable_constraints
                .into_iter()
                .map(|line| as_bullet(&line))
                .collect(),
            current_objective: vec![as_bullet(&current_objective)],
            committed_facts: wire
                .committed_facts
                .into_iter()
                .map(|line| as_bullet(&line))
                .collect(),
            uncommitted_working_set: wire
                .uncommitted_working_set
                .into_iter()
                .map(|line| as_bullet(&line))
                .collect(),
            open_decisions_and_risks: wire
                .open_decisions_and_risks
                .into_iter()
                .map(|line| as_bullet(&line))
                .collect(),
            resume_cursor_lines,
            next_action: wire.resume_cursor.next_action,
            required_revalidation: wire
                .required_revalidation
                .into_iter()
                .map(|line| as_bullet(&line))
                .collect(),
            archived_milestones: wire
                .archived_milestones
                .into_iter()
                .map(|line| as_bullet(&line))
                .collect(),
            status: wire.continuation_status,
            status_reason: Some(wire.continuation_reason),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointSections {
    pub immutable_constraints: Vec<String>,
    pub current_objective: Vec<String>,
    pub committed_facts: Vec<String>,
    pub uncommitted_working_set: Vec<String>,
    pub open_decisions_and_risks: Vec<String>,
    pub resume_cursor_lines: Vec<String>,
    pub next_action: String,
    pub required_revalidation: Vec<String>,
    pub archived_milestones: Vec<String>,
    pub status: ContinuationStatus,
    pub status_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationCheckpoint {
    sections: [Vec<String>; 9],
    resume_cursor: ResumeCursor,
    status: ContinuationStatus,
}

impl ContinuationCheckpoint {
    pub fn to_wire(&self) -> ContinuationCheckpointWire {
        let status_line = self.sections[8]
            .iter()
            .find(|line| !line.trim().is_empty())
            .cloned()
            .unwrap_or_default();
        let status_word = match self.status {
            ContinuationStatus::Continue => "Continue",
            ContinuationStatus::WaitingForUser => "Waiting for User",
            ContinuationStatus::Completed => "Completed",
        };
        let continuation_reason = status_line
            .strip_prefix(status_word)
            .unwrap_or(&status_line)
            .trim_start_matches(" —")
            .trim()
            .to_string();
        let resume_context = self.sections[5]
            .iter()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.starts_with("- Next action:") && !trimmed.starts_with("- Prohibited:")
            })
            .map(|line| without_bullet(line).to_string())
            .collect();
        let prohibited_actions = prohibited_lines(&self.sections[5])
            .into_iter()
            .map(|line| {
                line.trim_start()
                    .trim_start_matches("- Prohibited:")
                    .trim()
                    .to_string()
            })
            .collect();
        ContinuationCheckpointWire {
            immutable_constraints: section_without_bullets(&self.sections[0]),
            current_objective: self.sections[1]
                .first()
                .map(|line| without_bullet(line).to_string())
                .unwrap_or_default(),
            committed_facts: section_without_bullets(&self.sections[2]),
            uncommitted_working_set: section_without_bullets(&self.sections[3]),
            open_decisions_and_risks: section_without_bullets(&self.sections[4]),
            resume_cursor: ResumeCursorWire {
                context: resume_context,
                next_action: self.resume_cursor.next_action.clone(),
                prohibited_actions,
            },
            required_revalidation: section_without_bullets(&self.sections[6]),
            archived_milestones: section_without_bullets(&self.sections[7]),
            continuation_status: self.status,
            continuation_reason,
        }
    }

    pub fn from_sections(parts: CheckpointSections) -> Result<Self, CheckpointError> {
        let next_action = normalize_control_text(&parts.next_action);
        if next_action.is_empty() {
            return Err(CheckpointError::InvalidResumeCursor {
                next_action_count: 0,
            });
        }
        let mut resume_cursor_lines = parts.resume_cursor_lines;
        resume_cursor_lines.retain(|line| !line.trim_start().starts_with("- Next action:"));
        resume_cursor_lines.push(format!("- Next action: {next_action}"));
        let status_word = match parts.status {
            ContinuationStatus::Continue => "Continue",
            ContinuationStatus::WaitingForUser => "Waiting for User",
            ContinuationStatus::Completed => "Completed",
        };
        let status_line = parts
            .status_reason
            .filter(|reason| !reason.trim().is_empty())
            .map_or_else(
                || status_word.to_string(),
                |reason| format!("{status_word} — {}", normalize_control_text(&reason)),
            );
        let mut sections = [
            parts.immutable_constraints,
            parts.current_objective,
            parts.committed_facts,
            parts.uncommitted_working_set,
            parts.open_decisions_and_risks,
            resume_cursor_lines,
            parts.required_revalidation,
            parts.archived_milestones,
            vec![status_line],
        ];
        for section in &mut sections {
            *section = section
                .iter()
                .flat_map(|content| content.lines().map(str::to_string))
                .collect();
            trim_blank_lines(section);
        }
        Ok(Self {
            sections,
            resume_cursor: ResumeCursor {
                next_action: next_action.to_string(),
            },
            status: parts.status,
        })
    }

    pub fn from_legacy_summary(source: &str) -> Self {
        let legacy_sections = parse_legacy_sections(source);
        let current_objective = legacy_sections
            .get("User Requests")
            .or_else(|| legacy_sections.get("Goal"))
            .cloned()
            .unwrap_or_else(|| vec!["- Unknown legacy objective.".to_string()]);
        let legacy_next_action = legacy_sections
            .get("Next Action")
            .and_then(|lines| lines.iter().find(|line| !line.trim().is_empty()))
            .map(|line| line.trim_start_matches("- ").to_string())
            .unwrap_or_else(|| "Wait for a new user instruction.".to_string());
        let legacy_status = legacy_sections
            .get("Continuation Status")
            .and_then(|lines| lines.iter().find(|line| !line.trim().is_empty()))
            .and_then(|line| ContinuationStatus::parse(line).ok())
            .unwrap_or(ContinuationStatus::WaitingForUser);
        let legacy_working_set = if source.trim().is_empty() {
            "- None established from the legacy summary.".to_string()
        } else {
            format!("- unverified legacy summary: {}", source.replace('\n', " "))
        };
        Self::from_sections(CheckpointSections {
            immutable_constraints: vec![
                "- Preserve the action level of the legacy user request.".to_string(),
            ],
            current_objective,
            committed_facts: vec![
                "- None established from the legacy summary.".to_string(),
            ],
            uncommitted_working_set: vec![legacy_working_set],
            open_decisions_and_risks: vec![
                "- unverified legacy summary: facts and completion claims require confirmation."
                    .to_string(),
            ],
            resume_cursor_lines: vec![
                "- Prohibited: do not widen the legacy action level.".to_string(),
            ],
            next_action: legacy_next_action,
            required_revalidation: vec![
                "- Revalidate all dynamic Git, GitHub, CI, and worktree state from the legacy summary."
                    .to_string(),
            ],
            archived_milestones: vec![
                "- No stable milestone references recovered.".to_string(),
            ],
            status: legacy_status,
            status_reason: Some("migrated from legacy summary.".to_string()),
        })
        .expect("legacy checkpoint typed fields must be valid")
    }

    pub fn parse(source: &str) -> Result<Self, CheckpointError> {
        let mut sections: [Vec<String>; 9] = std::array::from_fn(|_| Vec::new());
        let mut seen = [false; 9];
        let mut encountered_sections = Vec::with_capacity(9);
        let mut current_section = None;

        for line in source.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                let Some(index) = SECTION_HEADINGS
                    .iter()
                    .position(|candidate| candidate == &heading)
                else {
                    return Err(CheckpointError::UnknownSection {
                        section: heading.to_string(),
                    });
                };
                if seen[index] {
                    return Err(CheckpointError::DuplicateSection {
                        section: SECTION_HEADINGS[index],
                    });
                }
                seen[index] = true;
                encountered_sections.push(index);
                current_section = Some(index);
                continue;
            }

            if let Some(index) = current_section {
                sections[index].push(decode_content_line(line));
            } else if !line.trim().is_empty() {
                return Err(CheckpointError::ContentBeforeFirstSection);
            }
        }

        if let Some(index) = seen.iter().position(|present| !present) {
            return Err(CheckpointError::MissingSection {
                section: SECTION_HEADINGS[index],
            });
        }
        if let Some((position, actual)) = encountered_sections
            .iter()
            .copied()
            .enumerate()
            .find(|(position, actual)| position != actual)
        {
            return Err(CheckpointError::InvalidSectionOrder {
                expected: SECTION_HEADINGS[position],
                actual: SECTION_HEADINGS[actual],
            });
        }

        for section in &mut sections {
            trim_blank_lines(section);
        }

        let next_actions = sections[5]
            .iter()
            .filter_map(|line| {
                line.trim_start()
                    .strip_prefix("- Next action:")
                    .map(str::trim)
            })
            .collect::<Vec<_>>();
        if next_actions.len() != 1 || next_actions[0].is_empty() {
            return Err(CheckpointError::InvalidResumeCursor {
                next_action_count: next_actions.len(),
            });
        }
        let resume_cursor = ResumeCursor {
            next_action: next_actions[0].to_string(),
        };

        let status_line = sections[8]
            .iter()
            .find(|line| !line.trim().is_empty())
            .ok_or_else(|| CheckpointError::InvalidStatus {
                value: String::new(),
            })?;
        let status = ContinuationStatus::parse(status_line)?;

        Ok(Self {
            sections,
            resume_cursor,
            status,
        })
    }

    pub fn apply_compression_patch(
        self,
        patch: CheckpointCompressionPatch,
    ) -> Result<Self, CheckpointError> {
        let protected_wire = self.to_wire();
        let patched = Self::try_from(ContinuationCheckpointWire {
            immutable_constraints: protected_wire.immutable_constraints,
            current_objective: protected_wire.current_objective,
            committed_facts: patch.committed_facts,
            uncommitted_working_set: patch.uncommitted_working_set,
            open_decisions_and_risks: patch.open_decisions_and_risks,
            resume_cursor: ResumeCursorWire {
                context: patch.resume_context,
                next_action: protected_wire.resume_cursor.next_action,
                prohibited_actions: protected_wire.resume_cursor.prohibited_actions,
            },
            required_revalidation: patch.required_revalidation,
            archived_milestones: patch.archived_milestones,
            continuation_status: protected_wire.continuation_status,
            continuation_reason: protected_wire.continuation_reason,
        })?;
        patched.validate_refresh_from(&self)?;
        Ok(patched)
    }

    pub fn status(&self) -> ContinuationStatus {
        self.status
    }

    pub fn resume_cursor(&self) -> &ResumeCursor {
        &self.resume_cursor
    }

    pub fn merge_fallback_update(mut self, current: Self) -> Self {
        for section_index in [0usize, 2, 3, 4, 6, 7] {
            self.sections[section_index].extend(current.sections[section_index].clone());
        }
        self.sections[1] = current.sections[1].clone();
        self.sections[5] = current.sections[5].clone();
        self.sections[8] = current.sections[8].clone();
        self.resume_cursor = current.resume_cursor;
        self.status = current.status;
        self.remove_duplicate_lines();
        self.compact_archived_milestones();
        self
    }

    pub fn normalize_to_budget(mut self, budget: usize) -> Result<Self, CheckpointError> {
        self.move_dynamic_facts_to_revalidation();
        self.remove_duplicate_lines();
        self.compact_archived_milestones();

        if estimate_checkpoint_tokens(&self) <= budget {
            return Ok(self);
        }

        for section_index in [7usize, 2, 3, 4] {
            while !self.sections[section_index].is_empty()
                && estimate_checkpoint_tokens(&self) > budget
            {
                self.sections[section_index].pop();
            }
        }

        let estimated_tokens = estimate_checkpoint_tokens(&self);
        if estimated_tokens > budget {
            return Err(CheckpointError::ProtectedSectionsExceedBudget {
                estimated_tokens,
                budget,
            });
        }
        Ok(self)
    }

    fn move_dynamic_facts_to_revalidation(&mut self) {
        let mut stable_facts = Vec::new();
        let mut revalidation = Vec::new();
        for line in self.sections[2].drain(..) {
            if is_dynamic_current_state(&line) {
                let content = line.trim_start().strip_prefix("- ").unwrap_or(&line);
                revalidation.push(format!("- Revalidate: {content}"));
            } else {
                stable_facts.push(line);
            }
        }
        self.sections[2] = stable_facts;
        self.sections[6].extend(revalidation);
    }

    fn remove_duplicate_lines(&mut self) {
        let mut seen = std::collections::HashSet::new();
        for section in &mut self.sections {
            section.retain(|line| {
                let normalized = line.trim();
                normalized.is_empty() || seen.insert(normalized.to_string())
            });
        }
    }

    fn compact_archived_milestones(&mut self) {
        self.sections[7].retain(|line| {
            let trimmed = line.trim();
            trimmed.is_empty()
                || trimmed.contains('`')
                || trimmed.contains("PR #")
                || trimmed.contains("Issue #")
        });
    }

    pub fn validate_refresh_from(&self, previous: &Self) -> Result<(), CheckpointError> {
        let protected_sections_match = self.sections[0] == previous.sections[0]
            && self.sections[1] == previous.sections[1]
            && prohibited_lines(&self.sections[5]) == prohibited_lines(&previous.sections[5])
            && self.resume_cursor == previous.resume_cursor
            && self.status == previous.status
            && self.sections[8] == previous.sections[8];
        if protected_sections_match {
            Ok(())
        } else {
            Err(CheckpointError::ProtectedRefreshChanged)
        }
    }

    pub fn render(&self) -> String {
        SECTION_HEADINGS
            .iter()
            .enumerate()
            .map(|(index, heading)| {
                let content = self.sections[index]
                    .iter()
                    .map(|line| encode_content_line(line))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("## {heading}\n{content}")
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

const CONTENT_ESCAPE_PREFIX: &str = "\\";

fn encode_content_line(line: &str) -> String {
    if line.starts_with("## ") || line.starts_with(CONTENT_ESCAPE_PREFIX) {
        format!("{CONTENT_ESCAPE_PREFIX}{line}")
    } else {
        line.to_string()
    }
}

fn decode_content_line(line: &str) -> String {
    line.strip_prefix(CONTENT_ESCAPE_PREFIX)
        .filter(|content| content.starts_with("## ") || content.starts_with(CONTENT_ESCAPE_PREFIX))
        .unwrap_or(line)
        .to_string()
}

fn section_without_bullets(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| without_bullet(line).to_string())
        .collect()
}

fn without_bullet(line: &str) -> &str {
    line.trim().strip_prefix("- ").unwrap_or(line.trim())
}

fn prohibited_lines(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .map(String::as_str)
        .filter(|line| line.trim_start().starts_with("- Prohibited:"))
        .collect()
}

fn as_bullet(source: &str) -> String {
    let normalized = normalize_control_text(source);
    if normalized.starts_with("- ") {
        normalized
    } else {
        format!("- {normalized}")
    }
}

fn normalize_control_text(source: &str) -> String {
    source.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

pub fn split_checkpoint_and_task_state(source: &str) -> (&str, Option<&str>) {
    const TASK_HEADING: &str = "\n\n## Current Task State\n";
    source
        .rsplit_once(TASK_HEADING)
        .map_or((source, None), |(checkpoint, task_state)| {
            (checkpoint, Some(task_state.trim()))
        })
}

fn parse_legacy_sections(source: &str) -> std::collections::HashMap<&str, Vec<String>> {
    let mut sections = std::collections::HashMap::new();
    let mut current_heading = None;
    for line in source.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            current_heading = Some(heading);
            sections.entry(heading).or_insert_with(Vec::new);
        } else if let Some(heading) = current_heading {
            sections
                .entry(heading)
                .or_insert_with(Vec::new)
                .push(line.to_string());
        }
    }
    for lines in sections.values_mut() {
        trim_blank_lines(lines);
    }
    sections
}

fn estimate_checkpoint_tokens(checkpoint: &ContinuationCheckpoint) -> usize {
    crate::domain::token_budget::estimate_tokens(&checkpoint.render())
}

fn is_dynamic_current_state(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    (lower.contains("pr #")
        && (lower.contains(" is open")
            || lower.contains(" is closed")
            || lower.contains("mergeable")))
        || lower.contains("ci is ")
        || lower.contains("worktree is ")
        || lower.contains("origin branch matches")
        || lower.contains("remote branch")
}

fn trim_blank_lines(lines: &mut Vec<String>) {
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointError {
    MissingSection {
        section: &'static str,
    },
    DuplicateSection {
        section: &'static str,
    },
    InvalidSectionOrder {
        expected: &'static str,
        actual: &'static str,
    },
    UnknownSection {
        section: String,
    },
    InvalidResumeCursor {
        next_action_count: usize,
    },
    InvalidStatus {
        value: String,
    },
    ProtectedSectionsExceedBudget {
        estimated_tokens: usize,
        budget: usize,
    },
    ProtectedRefreshChanged,
    MissingCurrentObjective,
    ContentBeforeFirstSection,
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSection { section } => write!(formatter, "缺少必需分区：{section}"),
            Self::DuplicateSection { section } => write!(formatter, "存在重复分区：{section}"),
            Self::InvalidSectionOrder { expected, actual } => write!(
                formatter,
                "checkpoint 分区顺序错误：期望 {expected}，实际 {actual}"
            ),
            Self::UnknownSection { section } => write!(formatter, "存在未知分区：{section}"),
            Self::InvalidResumeCursor { next_action_count } => write!(
                formatter,
                "Resume Cursor 必须包含唯一 Next action，实际 {next_action_count} 个"
            ),
            Self::InvalidStatus { value } => write!(
                formatter,
                "Continuation Status 非法：{value}；只允许 Continue、Waiting for User 或 Completed"
            ),
            Self::ProtectedSectionsExceedBudget {
                estimated_tokens,
                budget,
            } => write!(
                formatter,
                "checkpoint 保护分区超过预算：估算 {estimated_tokens} tokens，预算 {budget} tokens"
            ),
            Self::ProtectedRefreshChanged => {
                write!(formatter, "refresh 修改了受保护的 continuation 语义")
            }
            Self::MissingCurrentObjective => write!(formatter, "Current Objective 不得为空"),
            Self::ContentBeforeFirstSection => {
                write!(formatter, "首个 checkpoint 分区前存在非法内容")
            }
        }
    }
}

impl std::error::Error for CheckpointError {}

#[cfg(test)]
#[path = "continuation_checkpoint_tests.rs"]
mod continuation_checkpoint_tests;
