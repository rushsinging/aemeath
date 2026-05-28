#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionMetadata {
    pub id: String,
    pub message_count: usize,
}
