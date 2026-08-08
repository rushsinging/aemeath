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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationCheckpoint {
    sections: [Vec<String>; 9],
    resume_cursor: ResumeCursor,
    status: ContinuationStatus,
}

impl ContinuationCheckpoint {
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
        let status_text = match legacy_status {
            ContinuationStatus::Continue => "Continue — migrated from legacy summary.",
            ContinuationStatus::WaitingForUser => {
                "Waiting for User — migrated from legacy summary."
            }
            ContinuationStatus::Completed => "Completed — migrated from legacy summary.",
        };
        let source = format!(
            "## Immutable Constraints\n- Preserve the action level of the legacy user request.\n\n\
             ## Current Objective\n{}\n\n\
             ## Committed Facts\n- None established from the legacy summary.\n\n\
             ## Uncommitted Working Set\n- None established from the legacy summary.\n\n\
             ## Open Decisions / Risks\n- unverified legacy summary: facts and completion claims require confirmation.\n\n\
             ## Resume Cursor\n- Next action: {legacy_next_action}\n- Prohibited: do not widen the legacy action level.\n\n\
             ## Required Revalidation\n- Revalidate all dynamic Git, GitHub, CI, and worktree state from the legacy summary.\n\n\
             ## Archived Milestones\n- No stable milestone references recovered.\n\n\
             ## Continuation Status\n{status_text}",
            current_objective.join("\n")
        );
        Self::parse(&source).expect("constructed legacy checkpoint must be valid")
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
                sections[index].push(line.to_string());
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

    pub fn status(&self) -> ContinuationStatus {
        self.status
    }

    pub fn resume_cursor(&self) -> &ResumeCursor {
        &self.resume_cursor
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

    pub fn render(&self) -> String {
        SECTION_HEADINGS
            .iter()
            .enumerate()
            .map(|(index, heading)| {
                let content = self.sections[index].join("\n");
                format!("## {heading}\n{content}")
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
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
