pub mod agent_event;
pub mod effect_result;
pub mod input;
pub mod input_widget;
pub mod key_event;
pub mod live_status_widget;
// mouse_event adapter 为后续输入单源迁移子任务预备的脚手架，当前未接线消费。
#[allow(dead_code)]
pub mod mouse_event;
pub mod output_view_widget;
pub mod output_widget;
pub mod resize;
pub mod status_widget;
