#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionIntent {
    SetCurrentSession { id: String },
    MessagesSynced { message_count: usize },
    MessageStateChanged { message_count: usize, revision: u64 },
}
