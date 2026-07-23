//! 输入相关纯数据状态

/// 输入框的所有可变数据（不含视图组件 InputArea）
#[derive(Default)]
pub(crate) struct InputState {
    pub just_pasted: bool,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
}
