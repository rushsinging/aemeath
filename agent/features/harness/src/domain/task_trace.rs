//! 任务轨迹：把 session 中的工具调用还原成可解释的任务过程。
//!
//! 与工具指纹链不同，TaskTrace 保留用户目标、修改文件、验证和终态。
//! 只有具备任务边界与验证证据的轨迹才能进入 skill 生成。

use crate::domain::evidence::{EvidenceSignal, EvidenceSourceRef, SignalKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskActionKind {
    Read,
    Edit,
    Write,
    Test,
    Git,
    Pr,
    Ci,
    Other,
}

impl TaskActionKind {
    pub fn from_tool_command(tool: &str, command: &str) -> Self {
        let normalized = command.to_ascii_lowercase();
        if tool == "Edit" {
            return Self::Edit;
        }
        if tool == "Write" {
            return Self::Write;
        }
        if tool == "Bash"
            && (normalized.contains(".write_text(")
                || normalized.contains("cat > ")
                || normalized.contains("tee "))
        {
            return Self::Edit;
        }
        if tool == "Read" || tool == "Grep" || tool == "Glob" {
            return Self::Read;
        }
        if normalized.contains("cargo test")
            || normalized.contains("cargo clippy")
            || normalized.contains("cargo fmt")
            || normalized.contains("cargo xtask")
            || normalized.contains("cargo run -p xtask")
            || normalized.contains("xtask guard")
            || normalized.contains("pnpm test")
            || normalized.contains("npm test")
            || normalized.contains("pytest")
            || normalized.contains("go test")
        {
            return Self::Test;
        }
        if normalized.contains("gh pr") {
            if normalized.contains("checks")
                || normalized.contains("mergestate")
                || normalized.contains("ci_clean")
            {
                return Self::Ci;
            }
            return Self::Pr;
        }
        if normalized.contains("git push") || normalized.contains("git fetch") {
            return Self::Git;
        }
        Self::Other
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAction {
    pub kind: TaskActionKind,
    pub tool: String,
    pub command: Option<String>,
    /// 命令退出码或工具错误可证明的结果；普通工具返回不代表任务成功。
    pub success: Option<bool>,
    pub target_path: Option<String>,
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationFact {
    pub command: String,
    pub success: Option<bool>,
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTrace {
    pub session_id: String,
    pub run_id: String,
    pub user_goal: String,
    pub changed_files: Vec<String>,
    pub actions: Vec<TaskAction>,
    pub verification: Vec<VerificationFact>,
    pub outcome: String,
    pub sources: Vec<EvidenceSourceRef>,
}

impl TaskTrace {
    pub fn trace_id(&self) -> String {
        format!("{}#{}", self.session_id, self.run_id)
    }

    fn has_action(&self, predicate: impl Fn(&TaskAction) -> bool) -> bool {
        self.actions.iter().any(predicate)
    }

    /// 候选分数：要求有目标、实际修改和验证，不接受纯工具重复链。
    /// 低分/负分代表不应进入 skill 生成。
    pub fn candidate_score(&self) -> i32 {
        if self.user_goal.trim().is_empty() {
            return -10;
        }
        let has_edit = self.has_action(|action| {
            matches!(action.kind, TaskActionKind::Edit | TaskActionKind::Write)
        });
        let has_test = self
            .verification
            .iter()
            .any(|fact| fact.success == Some(true));
        let has_git = self.has_action(|action| action.kind == TaskActionKind::Git);
        let has_pr = self.has_action(|action| action.kind == TaskActionKind::Pr);
        let has_ci = self.has_action(|action| action.kind == TaskActionKind::Ci);
        if !has_edit || !has_test {
            return -5;
        }
        let mut score = 2 + 3 + 3;
        score += i32::from(has_git) * 2;
        score += i32::from(has_pr) * 2;
        score += i32::from(has_ci) * 2;
        if self.actions.len() >= 3 {
            score += 1;
        }
        if self.outcome.contains("Cancel") || self.outcome.contains("cancel") {
            score -= 3;
        }
        score
    }

    pub fn has_sensitive_path_or_secret(&self) -> bool {
        let mut haystack = self.user_goal.clone();
        for action in &self.actions {
            if let Some(command) = &action.command {
                haystack.push(' ');
                haystack.push_str(command);
            }
        }
        let lower = haystack.to_ascii_lowercase();
        [
            "sk-",
            "bearer ",
            "api_key",
            "api-key",
            "password",
            "secret",
            "private key",
            "/users/",
            ".ark_api_key",
            "token=",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    }
}

fn skill_name_from_action(action: &TaskAction) -> Option<String> {
    if action.tool != "Skill" {
        return None;
    }
    let input: serde_json::Value = serde_json::from_str(action.command.as_deref()?).ok()?;
    input.get("skill")?.as_str().map(str::to_string)
}

/// 把 Skill 调用之后的验证命令转成效果信号。
/// 加载前的测试不计入；同一 Skill 以最后一次有效结果为准。
pub fn skill_effect_signals(traces: &[TaskTrace]) -> Vec<EvidenceSignal> {
    let mut latest = std::collections::BTreeMap::<String, (bool, Option<EvidenceSourceRef>)>::new();
    for trace in traces {
        for (index, action) in trace.actions.iter().enumerate() {
            let Some(skill_name) = skill_name_from_action(action) else {
                continue;
            };
            let verification = trace.actions[index + 1..]
                .iter()
                .filter(|next| next.kind == TaskActionKind::Test)
                .collect::<Vec<_>>();
            if verification.iter().any(|next| next.success == Some(false)) {
                latest.insert(skill_name, (false, trace.sources.first().cloned()));
            } else if verification.iter().any(|next| next.success == Some(true)) {
                latest.insert(skill_name, (true, trace.sources.first().cloned()));
            }
        }
    }
    latest
        .into_iter()
        .map(|(skill_name, (verified, source))| EvidenceSignal {
            kind: SignalKind::SkillEffect,
            signal_id: format!(
                "skill_effect:{skill_name}:{}",
                if verified { "verified" } else { "ineffective" }
            ),
            summary: format!(
                "Skill {skill_name} 使用后{}",
                if verified {
                    "验证通过"
                } else {
                    "验证失败"
                }
            ),
            excerpt: None,
            occurrences: 1,
            sources: source.into_iter().collect(),
        })
        .collect()
}
