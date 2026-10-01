use super::*;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;

#[test]
fn test_records_notice() {
    let mut model = DiagnosticModel::default();
    let changes = model.apply(DiagnosticIntent::RecordNotice {
        severity: DiagnosticSeverity::Warning,
        message: "late event".to_string(),
    });
    assert_eq!(model.notices.len(), 1);
    assert!(changes
        .iter()
        .any(|change| matches!(change, DiagnosticChange::NoticeRecorded { .. })));
}

#[test]
fn test_highest_severity_prefers_error() {
    let mut model = DiagnosticModel::default();
    model.apply(DiagnosticIntent::RecordNotice {
        severity: DiagnosticSeverity::Info,
        message: "info".to_string(),
    });
    model.apply(DiagnosticIntent::RecordNotice {
        severity: DiagnosticSeverity::Error,
        message: "error".to_string(),
    });
    assert_eq!(model.highest_severity(), Some(DiagnosticSeverity::Error));
}
