use sdk::ChatInputEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRunInput {
    pub id: sdk::InputId,
    pub text: String,
    pub images: Vec<sdk::ChatInputImage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeControl {
    Command(String),
    Reset,
    WithdrawAll,
    Compact,
    SwitchModel(String),
    SetThinking(Option<bool>),
    InitProject(bool),
    ManageSession(String),
    ManageMemory(String),
    ResumeSession(String),
    RunReflection,
    ApplyReflection(sdk::ReflectionOutputView),
    ListModels,
    ListReminders,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeInputBatch {
    pub user_inputs: Vec<UserRunInput>,
    pub controls: Vec<RuntimeControl>,
}

pub fn split_input_events(events: impl IntoIterator<Item = ChatInputEvent>) -> RuntimeInputBatch {
    let mut batch = RuntimeInputBatch::default();
    for event in events {
        match event {
            ChatInputEvent::UserMessage { id, text, images } => {
                batch.user_inputs.push(UserRunInput { id, text, images });
            }
            ChatInputEvent::ControlCommand { raw } => {
                batch.controls.push(RuntimeControl::Command(raw));
            }
            ChatInputEvent::Reset => batch.controls.push(RuntimeControl::Reset),
            ChatInputEvent::WithdrawAll => batch.controls.push(RuntimeControl::WithdrawAll),
            ChatInputEvent::Compact => batch.controls.push(RuntimeControl::Compact),
            ChatInputEvent::SwitchModel { selection } => {
                batch.controls.push(RuntimeControl::SwitchModel(selection));
            }
            ChatInputEvent::SetThinking { desired } => {
                batch.controls.push(RuntimeControl::SetThinking(desired));
            }
            ChatInputEvent::InitProject { force } => {
                batch.controls.push(RuntimeControl::InitProject(force));
            }
            ChatInputEvent::ManageSession { args } => {
                batch.controls.push(RuntimeControl::ManageSession(args));
            }
            ChatInputEvent::ManageMemory { args } => {
                batch.controls.push(RuntimeControl::ManageMemory(args));
            }
            ChatInputEvent::ResumeSession { id } => {
                batch.controls.push(RuntimeControl::ResumeSession(id));
            }
            ChatInputEvent::RunReflection => batch.controls.push(RuntimeControl::RunReflection),
            ChatInputEvent::ApplyReflection { output } => {
                batch.controls.push(RuntimeControl::ApplyReflection(output));
            }
            ChatInputEvent::ListModels => batch.controls.push(RuntimeControl::ListModels),
            ChatInputEvent::ListReminders => batch.controls.push(RuntimeControl::ListReminders),
        }
    }
    batch
}
