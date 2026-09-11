use std::collections::{HashMap, HashSet};

use super::{
    ConfigFormCommand, ConfigFormError, ConfigFormFieldType, ConfigFormFieldValue,
    ConfigFormOrigin, ConfigFormPage, ConfigFormRevision, ConfigFormSessionId, ConfigFormValue,
    ConfigFormView, ConfigFormWorkflowId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedConfigFormPage {
    pub values: Vec<ConfigFormFieldValue>,
}

#[derive(Default)]
pub struct ConfigFormService {
    sessions: HashMap<ConfigFormSessionId, ConfigFormView>,
}

impl ConfigFormService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(
        &mut self,
        workflow_id: ConfigFormWorkflowId,
        origin: ConfigFormOrigin,
        page: ConfigFormPage,
    ) -> ConfigFormView {
        let view = ConfigFormView {
            workflow_id,
            session_id: ConfigFormSessionId::next(),
            revision: ConfigFormRevision::initial(),
            origin,
            page,
            busy: None,
            terminal: None,
        };
        self.sessions.insert(view.session_id.clone(), view.clone());
        view
    }

    pub fn view(&self, session_id: &ConfigFormSessionId) -> Option<ConfigFormView> {
        self.sessions.get(session_id).cloned()
    }

    pub fn apply(
        &mut self,
        session_id: ConfigFormSessionId,
        expected_revision: ConfigFormRevision,
        command: ConfigFormCommand,
    ) -> Result<ConfigFormView, ConfigFormError> {
        let current = self
            .sessions
            .get(&session_id)
            .cloned()
            .ok_or(ConfigFormError::SessionNotFound)?;
        if current.revision != expected_revision {
            return Err(ConfigFormError::StaleRevision {
                actual: current.revision,
                provided: expected_revision,
            });
        }

        let mut updated = current.clone();
        match command {
            ConfigFormCommand::SubmitPage { values } => {
                let submitted = validate_page_submission(&current.page, values)?;
                apply_redacted_field_presence(&mut updated.page, &submitted);
            }
            ConfigFormCommand::InvokeAction { action_id } => {
                if !current
                    .page
                    .actions
                    .iter()
                    .any(|action| action.id == action_id)
                {
                    return Err(ConfigFormError::UnknownAction { action_id });
                }
            }
            ConfigFormCommand::Back | ConfigFormCommand::Cancel | ConfigFormCommand::Refresh => {}
        }
        updated.revision = updated.revision.next();
        self.sessions.insert(session_id, updated.clone());
        Ok(updated)
    }
}

fn validate_page_submission(
    page: &ConfigFormPage,
    values: Vec<ConfigFormFieldValue>,
) -> Result<SubmittedConfigFormPage, ConfigFormError> {
    let mut submitted_fields = HashSet::new();
    for submitted in &values {
        if !submitted_fields.insert(submitted.field_id.clone()) {
            return Err(ConfigFormError::DuplicateField {
                field_id: submitted.field_id.clone(),
            });
        }
        let field = page
            .fields
            .iter()
            .find(|field| field.id == submitted.field_id)
            .ok_or_else(|| ConfigFormError::UnknownField {
                field_id: submitted.field_id.clone(),
            })?;
        if matches!(
            field.field_type,
            ConfigFormFieldType::Summary | ConfigFormFieldType::Status
        ) {
            return Err(ConfigFormError::ReadOnlyField {
                field_id: submitted.field_id.clone(),
            });
        }
        let actual = submitted.value.field_type();
        if actual != field.field_type {
            return Err(ConfigFormError::InvalidValueType {
                field_id: submitted.field_id.clone(),
                expected: field.field_type,
                actual,
            });
        }
        if let ConfigFormValue::SelectedOption(option_id) = &submitted.value {
            if !field.options.iter().any(|option| option.id == *option_id) {
                return Err(ConfigFormError::UnknownOption {
                    field_id: submitted.field_id.clone(),
                    option_id: option_id.clone(),
                });
            }
        }
    }

    for field in page.fields.iter().filter(|field| field.required) {
        if !submitted_fields.contains(&field.id) {
            return Err(ConfigFormError::MissingRequiredField {
                field_id: field.id.clone(),
            });
        }
    }
    Ok(SubmittedConfigFormPage { values })
}

fn apply_redacted_field_presence(page: &mut ConfigFormPage, submitted: &SubmittedConfigFormPage) {
    for value in &submitted.values {
        let Some(field) = page
            .fields
            .iter_mut()
            .find(|field| field.id == value.field_id)
        else {
            continue;
        };
        field.has_value = match &value.value {
            ConfigFormValue::Text(value) | ConfigFormValue::Secret(value) => !value.is_empty(),
            ConfigFormValue::Number(_)
            | ConfigFormValue::Boolean(_)
            | ConfigFormValue::SelectedOption(_) => true,
        };
        if field.field_type == ConfigFormFieldType::Secret {
            field.display_value = None;
        }
    }
}
