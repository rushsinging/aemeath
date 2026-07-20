use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::domain::{
    ApplicationControlCommand, ApplicationControlTarget, CommandArgumentSchema, CommandCatalogPort,
    CommandCompletion, CommandDescriptor, CommandMechanism, CommandName, CommandParseError,
    CommandRoute, CommandRouterPort, CommandTarget, ParsedArguments, PromptCommand, SlashInput,
    SnapshotQueryCommand, SnapshotQueryTarget,
};

#[derive(Clone)]
pub struct CommandAdapter {
    descriptors: Arc<Vec<CommandDescriptor>>,
    lookup: Arc<BTreeMap<String, usize>>,
}

impl CommandAdapter {
    pub fn try_new(mut descriptors: Vec<CommandDescriptor>) -> Result<Self, CommandParseError> {
        descriptors.sort_by(|left, right| left.name.cmp(&right.name));
        let mut lookup = BTreeMap::new();
        for (index, descriptor) in descriptors.iter().enumerate() {
            validate_target(descriptor)?;
            for name in std::iter::once(&descriptor.name).chain(descriptor.aliases.iter()) {
                if lookup.insert(name.as_str().to_string(), index).is_some() {
                    return Err(CommandParseError::DuplicateName {
                        name: name.as_str().to_string(),
                    });
                }
            }
        }
        Ok(Self {
            descriptors: Arc::new(descriptors),
            lookup: Arc::new(lookup),
        })
    }

    fn descriptor(&self, name: &str) -> Option<&CommandDescriptor> {
        self.lookup
            .get(name)
            .and_then(|index| self.descriptors.get(*index))
    }
}

impl CommandCatalogPort for CommandAdapter {
    fn list(&self) -> Vec<CommandDescriptor> {
        self.descriptors.as_ref().clone()
    }

    fn complete(&self, prefix: &str) -> Vec<CommandCompletion> {
        let search = prefix.trim().trim_start_matches('/').to_ascii_lowercase();
        let mut seen = BTreeSet::new();
        let mut completions = Vec::new();
        for descriptor in self.descriptors.iter() {
            for name in std::iter::once(&descriptor.name).chain(descriptor.aliases.iter()) {
                if name.as_str().starts_with(&search) && seen.insert(name.as_str().to_string()) {
                    let replacement = format!("/{}", name.as_str());
                    completions.push(CommandCompletion {
                        replacement: replacement.clone(),
                        display: replacement,
                        description: descriptor.description.clone(),
                    });
                }
            }
        }
        completions
    }
}

impl CommandRouterPort for CommandAdapter {
    fn resolve(&self, input: SlashInput) -> Result<CommandRoute, CommandParseError> {
        let mut parts = input.as_str().split_whitespace();
        let raw_name = parts.next().unwrap_or_default().trim_start_matches('/');
        let lookup_name = CommandName::new(raw_name)?;
        let descriptor = self.descriptor(lookup_name.as_str()).ok_or_else(|| {
            CommandParseError::UnknownCommand {
                name: lookup_name.as_str().to_string(),
            }
        })?;
        let arguments = parse_arguments(descriptor, parts.map(str::to_string).collect())?;
        let command = descriptor.name.clone();
        match descriptor.mechanism {
            CommandMechanism::PromptInjection => Ok(CommandRoute::PromptInjection(PromptCommand {
                command,
                arguments,
            })),
            CommandMechanism::SnapshotQuery => Ok(CommandRoute::SnapshotQuery {
                target: snapshot_target(descriptor.target).ok_or_else(|| {
                    CommandParseError::TargetMismatch {
                        command: descriptor.name.as_str().to_string(),
                    }
                })?,
                command: SnapshotQueryCommand { command, arguments },
            }),
            CommandMechanism::ApplicationControl => Ok(CommandRoute::ApplicationControl {
                target: control_target(descriptor.target).ok_or_else(|| {
                    CommandParseError::TargetMismatch {
                        command: descriptor.name.as_str().to_string(),
                    }
                })?,
                command: ApplicationControlCommand { command, arguments },
            }),
        }
    }
}

fn parse_arguments(
    descriptor: &CommandDescriptor,
    raw: Vec<String>,
) -> Result<ParsedArguments, CommandParseError> {
    match descriptor.argument_schema {
        CommandArgumentSchema::None if !raw.is_empty() => {
            Err(CommandParseError::UnexpectedArguments {
                command: descriptor.name.as_str().to_string(),
            })
        }
        CommandArgumentSchema::None | CommandArgumentSchema::OptionalText => {
            Ok(ParsedArguments::new(raw))
        }
        CommandArgumentSchema::RequiredText if raw.is_empty() => {
            Err(CommandParseError::MissingArgument {
                command: descriptor.name.as_str().to_string(),
            })
        }
        CommandArgumentSchema::RequiredText => Ok(ParsedArguments::new(raw)),
        CommandArgumentSchema::OptionalPositiveUsize { default } => match raw.as_slice() {
            [] => Ok(ParsedArguments::new(vec![default.to_string()])),
            [value] if value.parse::<usize>().is_ok_and(|parsed| parsed > 0) => {
                Ok(ParsedArguments::new(raw))
            }
            [value] => Err(CommandParseError::InvalidArgument {
                command: descriptor.name.as_str().to_string(),
                value: value.clone(),
            }),
            _ => Err(CommandParseError::UnexpectedArguments {
                command: descriptor.name.as_str().to_string(),
            }),
        },
    }
}

fn validate_target(descriptor: &CommandDescriptor) -> Result<(), CommandParseError> {
    let valid = match descriptor.mechanism {
        CommandMechanism::PromptInjection => true,
        CommandMechanism::SnapshotQuery => snapshot_target(descriptor.target).is_some(),
        CommandMechanism::ApplicationControl => control_target(descriptor.target).is_some(),
    };
    if valid {
        Ok(())
    } else {
        Err(CommandParseError::TargetMismatch {
            command: descriptor.name.as_str().to_string(),
        })
    }
}

fn snapshot_target(target: CommandTarget) -> Option<SnapshotQueryTarget> {
    Some(match target {
        CommandTarget::Runtime => SnapshotQueryTarget::Runtime,
        CommandTarget::ContextManagement => SnapshotQueryTarget::ContextManagement,
        CommandTarget::Memory => SnapshotQueryTarget::Memory,
        CommandTarget::Task => SnapshotQueryTarget::Task,
        CommandTarget::Project => SnapshotQueryTarget::Project,
        CommandTarget::Provider => SnapshotQueryTarget::Provider,
        CommandTarget::Config => SnapshotQueryTarget::Config,
        CommandTarget::Audit => SnapshotQueryTarget::Audit,
        CommandTarget::ApplicationShell => SnapshotQueryTarget::ApplicationShell,
        CommandTarget::ApplicationVersionControl => return None,
    })
}

fn control_target(target: CommandTarget) -> Option<ApplicationControlTarget> {
    Some(match target {
        CommandTarget::Runtime => ApplicationControlTarget::Runtime,
        CommandTarget::ContextManagement => ApplicationControlTarget::ContextManagement,
        CommandTarget::Memory => ApplicationControlTarget::Memory,
        CommandTarget::Task => ApplicationControlTarget::Task,
        CommandTarget::Project => ApplicationControlTarget::Project,
        CommandTarget::Config => ApplicationControlTarget::Config,
        CommandTarget::ApplicationVersionControl => {
            ApplicationControlTarget::ApplicationVersionControl
        }
        CommandTarget::ApplicationShell => ApplicationControlTarget::ApplicationShell,
        CommandTarget::Provider | CommandTarget::Audit => return None,
    })
}
