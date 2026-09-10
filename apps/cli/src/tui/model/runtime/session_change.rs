#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionChange {
    CurrentSessionChanged {
        id: String,
    },
    DirtyChanged {
        dirty: bool,
    },
    MessagesSynced {
        message_count: usize,
    },
    MessageStateObserved {
        message_count: usize,
        revision: u64,
        revision_gap: Option<u64>,
    },
}
