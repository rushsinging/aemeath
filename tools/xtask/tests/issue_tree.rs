use xtask::issue_tree::{IssueRecord, VerificationStatus};

fn issue(number: u64, state: &str, body: &str) -> IssueRecord {
    IssueRecord {
        number,
        state: state.into(),
        body: body.into(),
        sub_issues: vec![],
    }
}

#[test]
fn issue_tree_accepts_completed_leaf_with_evidence() {
    let leaf = issue(
        1013,
        "CLOSED",
        r#"
<!-- doc-code-verification-gate:v1 -->
## 开发前文档—代码差异
| ID | 状态 |
| D1 | 已对齐 |
## 实施结果
PR #1024，commit `abc12345`。
"#,
    );
    let root = IssueRecord {
        number: 677,
        state: "OPEN".into(),
        body: String::new(),
        sub_issues: vec![leaf],
    };
    assert_eq!(
        xtask::issue_tree::verify(&root).status,
        VerificationStatus::Passed
    );
}

#[test]
fn issue_tree_rejects_pending_and_unowned_deferral() {
    let leaf = issue(
        1018,
        "OPEN",
        r#"
<!-- doc-code-verification-gate:v1 -->
## 开发前文档—代码差异
| G1 | 待对齐 |
| G2 | 经确认延期 | 后续处理 |
"#,
    );
    let root = IssueRecord {
        number: 677,
        state: "OPEN".into(),
        body: String::new(),
        sub_issues: vec![leaf],
    };
    let report = xtask::issue_tree::verify(&root);
    assert_eq!(report.status, VerificationStatus::Failed);
    assert!(report.errors.iter().any(|error| error.contains("待对齐")));
    assert!(report
        .errors
        .iter()
        .any(|error| error.contains("承接 Issue")));
}
