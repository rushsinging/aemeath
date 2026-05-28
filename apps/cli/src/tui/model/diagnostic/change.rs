use super::notice::DiagnosticSeverity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticChange {
    NoticeRecorded {
        id: String,
        severity: DiagnosticSeverity,
    },
    PromptOpened {
        id: String,
    },
    PromptAnswered {
        answer: String,
    },
    NoticeDismissed {
        id: String,
    },
}
