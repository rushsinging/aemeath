/// 排队的控制类命令（#1816）。
///
/// `input_id` 是 runtime 分配的入队序号（UUIDv7），与消息占位的 `input_id`
/// 同源，因此两类占位可按它合并成提交顺序。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedCommand {
    pub input_id: String,
    pub text: String,
}

impl QueuedCommand {
    pub fn new(input_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            input_id: input_id.into(),
            text: text.into(),
        }
    }
}
