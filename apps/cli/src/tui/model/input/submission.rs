use super::attachment::InputAttachment;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSubmission {
    pub text: String,
    pub display_text: String,
    pub attachments: Vec<InputAttachment>,
}
