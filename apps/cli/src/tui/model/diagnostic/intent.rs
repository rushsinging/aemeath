use super::notice::DiagnosticSeverity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticIntent {
    RecordNotice {
        severity: DiagnosticSeverity,
        message: String,
    },
}
