#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessingJob {
    pub id: String,
    pub chat_id: Option<String>,
    pub status: ProcessingStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessingStatus {
    Starting,
    Running,
    Finishing,
    Finished,
    Failed,
}
