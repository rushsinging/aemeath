use super::attachment::InputAttachment;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSubmission {
    pub text: String,
    pub attachments: Vec<InputAttachment>,
}
