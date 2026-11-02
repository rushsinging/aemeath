use super::command_pl::{
    CommandCompletion, CommandDescriptor, CommandParseError, CommandRoute, SlashInput,
};

pub trait CommandCatalogPort: crate::sealed::Sealed + Send + Sync {
    fn list(&self) -> Vec<CommandDescriptor>;
    fn complete(&self, prefix: &str) -> Vec<CommandCompletion>;
}

pub trait CommandRouterPort: crate::sealed::Sealed + Send + Sync {
    fn resolve(&self, input: SlashInput) -> Result<CommandRoute, CommandParseError>;
}
