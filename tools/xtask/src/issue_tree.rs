use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

const MARKER: &str = "<!-- doc-code-verification-gate:v1 -->";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IssueRecord {
    pub number: u64,
    pub state: String,
    pub body: String,
    #[serde(default)]
    pub sub_issues: Vec<IssueRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    Passed,
    Failed,
}

pub struct VerificationReport {
    pub status: VerificationStatus,
    pub errors: Vec<String>,
}

pub fn verify(root: &IssueRecord) -> VerificationReport {
    let mut errors = Vec::new();
    for issue in &root.sub_issues {
        if !issue.body.contains(MARKER) {
            errors.push(format!("#{} 缺少 gate marker", issue.number));
        }
        if !issue.body.contains("开发前文档—代码差异") {
            errors.push(format!("#{} 缺少开发前差异", issue.number));
        }
        if issue.body.contains("待对齐") {
            errors.push(format!("#{} 仍有待对齐", issue.number));
        }
        if issue.state.eq_ignore_ascii_case("closed") {
            if !issue.body.contains("实施结果") {
                errors.push(format!("#{} 缺少实施结果", issue.number));
            }
            if !issue.body.contains("PR #") || !issue.body.contains("commit `") {
                errors.push(format!("#{} 缺少 PR/commit 证据", issue.number));
            }
        }
        for line in issue.body.lines().filter(|line| {
            line.contains("经确认延期")
                && line.trim_start().starts_with('|')
                && !line.contains("附承接 Issue")
        }) {
            if !line.contains('#') {
                errors.push(format!("#{} 延期项缺少承接 Issue: {line}", issue.number));
            }
        }
    }
    VerificationReport {
        status: if errors.is_empty() {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        },
        errors,
    }
}

pub fn load_from_github(repo: &str, root_number: u64) -> Result<IssueRecord> {
    let query = format!(
        r#"query {{ repository(owner: "{}", name: "{}") {{ issue(number: {}) {{ number state body subIssues(first: 50) {{ nodes {{ number state body }} }} }} }} }}"#,
        repo.split('/').next().context("repo owner missing")?,
        repo.split('/').nth(1).context("repo name missing")?,
        root_number
    );
    let output = Command::new("gh")
        .args(["api", "graphql", "-f", &format!("query={query}")])
        .output()
        .context("执行 gh 失败")?;
    if !output.status.success() {
        anyhow::bail!("gh 查询失败: {}", String::from_utf8_lossy(&output.stderr));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let issue = &value["data"]["repository"]["issue"];
    let children: Vec<IssueRecord> = serde_json::from_value(issue["subIssues"]["nodes"].clone())?;
    Ok(IssueRecord {
        number: issue["number"].as_u64().unwrap_or(root_number),
        state: issue["state"].as_str().unwrap_or("UNKNOWN").into(),
        body: issue["body"].as_str().unwrap_or_default().into(),
        sub_issues: children,
    })
}
