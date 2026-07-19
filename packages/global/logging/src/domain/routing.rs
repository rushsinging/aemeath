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
    Workflow,
    Context,
    Config,
    Memory,
    Task,
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
    Workflow,
    Context,
    Config,
    Memory,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetSpec {
    pub(crate) target: LogTarget,
    pub(crate) owner: ModuleOwner,
    pub(crate) sink: DiagnosticSinkId,
    pub(crate) file_name: &'static str,
}

const FALLBACK: TargetSpec = TargetSpec {
    target: LogTarget::new("aemeath"),
    owner: ModuleOwner::Shared,
    sink: DiagnosticSinkId::Fallback,
    file_name: "aemeath.log",
};

macro_rules! target {
    ($target:literal, $owner:ident, $sink:ident, $file:literal) => {
        TargetSpec {
            target: LogTarget::new($target),
            owner: ModuleOwner::$owner,
            sink: DiagnosticSinkId::$sink,
            file_name: $file,
        }
    };
}

const TARGETS: &[TargetSpec] = &[
    target!("aemeath:tui", Tui, Tui, "tui.log"),
    target!("aemeath:shared", Shared, Shared, "shared.log"),
    target!(
        "aemeath:composition",
        Composition,
        Composition,
        "composition.log"
    ),
    target!(
        "aemeath:llm-api-error",
        Provider,
        LlmApiError,
        "llm-api-error.log"
    ),
    target!(
        "aemeath:agent:provider",
        Provider,
        Provider,
        "agent-provider.log"
    ),
    target!(
        "aemeath:agent:runtime",
        Runtime,
        Runtime,
        "agent-runtime.log"
    ),
    target!("aemeath:agent:tools", Tools, Tools, "agent-tools.log"),
    target!("aemeath:agent:prompt", Prompt, Prompt, "agent-prompt.log"),
    target!("aemeath:agent:config", Config, Config, "agent-config.log"),
    target!("aemeath:agent:memory", Memory, Memory, "agent-memory.log"),
    target!("aemeath:agent:task", Task, Task, "agent-task.log"),
    target!("aemeath:agent:hook", Hook, Hook, "agent-hook.log"),
    target!(
        "aemeath:agent:storage",
        Storage,
        Storage,
        "agent-storage.log"
    ),
    target!(
        "aemeath:agent:project",
        Project,
        Project,
        "agent-project.log"
    ),
    target!("aemeath:agent:policy", Policy, Policy, "agent-policy.log"),
    target!(
        "aemeath:diagnostic:audit",
        Audit,
        AuditDiagnostic,
        "audit-diagnostic.log"
    ),
    target!("aemeath:agent:update", Update, Update, "agent-update.log"),
    target!(
        "aemeath:agent:workflow",
        Workflow,
        Workflow,
        "agent-workflow.log"
    ),
    target!("aemeath:context", Context, Context, "context.log"),
];

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
