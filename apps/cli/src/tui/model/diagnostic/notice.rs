#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticNotice {
    pub id: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}
