#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelRunOutcome {
    Accepted,
    AlreadyCancelling,
    AlreadyTerminal,
    NotFound,
}
