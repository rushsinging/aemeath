use super::notice::DiagnosticSeverity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticIntent {
    RecordNotice {
        severity: DiagnosticSeverity,
        message: String,
    },
    OpenPrompt {
        id: String,
        question: String,
    },
    AnswerPrompt {
        answer: String,
    },
    DismissNotice {
        id: String,
    },
}
