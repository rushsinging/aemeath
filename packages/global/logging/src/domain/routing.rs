use super::constants::{FALLBACK, TARGETS};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LogTarget(&'static str);

impl LogTarget {
    pub(crate) const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub(crate) const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ModuleOwner {
    Tui,
    Shared,
    Composition,
    Provider,
    Runtime,
    Tools,
    Prompt,
    Hook,
    Storage,
    Project,
    Policy,
    Audit,
    Update,
    Context,
    Config,
    Memory,
    TaskData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DiagnosticSinkId {
    Fallback,
    Tui,
    Shared,
    Composition,
    LlmApiError,
    Provider,
    Runtime,
    Tools,
    Prompt,
    Hook,
    Storage,
    Project,
    Policy,
    AuditDiagnostic,
    Update,
    Context,
    Config,
    Memory,
    TaskData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetSpec {
    pub(crate) target: LogTarget,
    pub(crate) owner: ModuleOwner,
    pub(crate) sink: DiagnosticSinkId,
    pub(crate) file_name: &'static str,
}

pub(crate) struct TargetCatalog;

impl TargetCatalog {
    pub(crate) const fn specs() -> &'static [TargetSpec] {
        TARGETS
    }

    pub(crate) const fn fallback() -> TargetSpec {
        FALLBACK
    }

    #[cfg(test)]
    pub(crate) fn exact(target: &str) -> Option<TargetSpec> {
        TARGETS
            .iter()
            .find(|spec| spec.target.as_str() == target)
            .copied()
    }

    pub(crate) fn route(target: &str) -> Option<TargetSpec> {
        route_specs(TARGETS, target)
    }
}

fn route_specs(specs: &[TargetSpec], target: &str) -> Option<TargetSpec> {
    specs
        .iter()
        .filter(|spec| legal_prefix(spec.target.as_str(), target))
        .max_by_key(|spec| spec.target.as_str().len())
        .copied()
}

fn legal_prefix(prefix: &str, target: &str) -> bool {
    target == prefix
        || target
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with(':'))
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod tests;
