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
    QueryReflectionHistory(usize),
    ListModels,
    ListReminders,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeInputBatch {
    pub user_inputs: Vec<UserRunInput>,
    pub controls: Vec<RuntimeControl>,
}

pub fn format_skill_request(request: &sdk::SkillRequest, language: &str) -> String {
    let zh = language.eq_ignore_ascii_case("zh") || language.starts_with("zh-");
    let mut lines = if zh {
        vec![
            "<skill-request>".to_string(),
            format!("用户请求使用 Skill：{}", request.skill),
        ]
    } else {
        vec![
            "<skill-request>".to_string(),
            format!("The user requests Skill: {}", request.skill),
        ]
    };
    if !request.arguments.is_empty() {
        lines.push(if zh {
            format!("参考参数：{}", request.arguments)
        } else {
            format!("Reference arguments: {}", request.arguments)
        });
    }
    lines.push(if zh {
        "请先调用 Skill 工具加载该 Skill，再结合参考参数理解并执行。".to_string()
    } else {
        "Call the Skill tool first to load this Skill, then interpret and execute it using the reference arguments.".to_string()
    });
    lines.push("</skill-request>".to_string());
    lines.join("\n")
}

pub fn split_input_events(events: impl IntoIterator<Item = ChatInputEvent>) -> RuntimeInputBatch {
    let mut batch = RuntimeInputBatch::default();
    for event in events {
        match event {
            ChatInputEvent::UserMessage { id, text, images } => {
                batch.user_inputs.push(UserRunInput { id, text, images });
            }
            ChatInputEvent::SkillRequest(request) => {
                batch.user_inputs.push(UserRunInput {
                    id: request.input_id.clone(),
                    text: format_skill_request(&request, "en"),
                    images: Vec::new(),
                });
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
            ChatInputEvent::QueryReflectionHistory { limit } => batch
                .controls
                .push(RuntimeControl::QueryReflectionHistory(limit)),
            ChatInputEvent::ListModels => batch.controls.push(RuntimeControl::ListModels),
            ChatInputEvent::ListReminders => batch.controls.push(RuntimeControl::ListReminders),
        }
    }
    batch
}

#[cfg(test)]
mod skill_request_tests {
    use super::format_skill_request;

    fn request(arguments: &str) -> sdk::SkillRequest {
        sdk::SkillRequest {
            input_id: sdk::InputId::new_v7(),
            skill: "release".to_string(),
            arguments: arguments.to_string(),
            raw_input: format!("/release {arguments}").trim().to_string(),
        }
    }

    #[test]
    fn formats_chinese_and_english_without_body_or_duplicate_arguments() {
        let zh = format_skill_request(&request("v1.2.3"), "zh");
        assert!(zh.contains("用户请求使用 Skill：release"));
        assert!(zh.contains("参考参数：v1.2.3"));
        assert_eq!(zh.matches("v1.2.3").count(), 1);
        assert!(!zh.contains("SKILL.md"));

        let en = format_skill_request(&request(""), "en");
        assert!(en.contains("The user requests Skill: release"));
        assert!(!en.contains("Reference arguments:"));
        assert!(en.contains("Call the Skill tool first"));
    }
}
