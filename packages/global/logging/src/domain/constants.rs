pub(crate) const NATIVE_STDERR_FILE: &str = "native-stderr.log";

use super::routing::{DiagnosticSinkId, LogTarget, ModuleOwner, TargetSpec};

pub(crate) const TARGETS: &[TargetSpec] = &[
    TargetSpec {
        target: LogTarget::new("aemeath:tui"),
        owner: ModuleOwner::Tui,
        sink: DiagnosticSinkId::Tui,
        file_name: "tui.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:shared"),
        owner: ModuleOwner::Shared,
        sink: DiagnosticSinkId::Shared,
        file_name: "shared.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:composition"),
        owner: ModuleOwner::Composition,
        sink: DiagnosticSinkId::Composition,
        file_name: "composition.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:llm-api-error"),
        owner: ModuleOwner::Provider,
        sink: DiagnosticSinkId::LlmApiError,
        file_name: "llm-api-error.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:provider"),
        owner: ModuleOwner::Provider,
        sink: DiagnosticSinkId::Provider,
        file_name: "agent-provider.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:runtime"),
        owner: ModuleOwner::Runtime,
        sink: DiagnosticSinkId::Runtime,
        file_name: "agent-runtime.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:tools"),
        owner: ModuleOwner::Tools,
        sink: DiagnosticSinkId::Tools,
        file_name: "agent-tools.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:prompt"),
        owner: ModuleOwner::Prompt,
        sink: DiagnosticSinkId::Prompt,
        file_name: "agent-prompt.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:config"),
        owner: ModuleOwner::Config,
        sink: DiagnosticSinkId::Config,
        file_name: "agent-config.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:memory"),
        owner: ModuleOwner::Memory,
        sink: DiagnosticSinkId::Memory,
        file_name: "agent-memory.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:task"),
        owner: ModuleOwner::TaskData,
        sink: DiagnosticSinkId::TaskData,
        file_name: "agent-task.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:hook"),
        owner: ModuleOwner::Hook,
        sink: DiagnosticSinkId::Hook,
        file_name: "agent-hook.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:storage"),
        owner: ModuleOwner::Storage,
        sink: DiagnosticSinkId::Storage,
        file_name: "agent-storage.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:project"),
        owner: ModuleOwner::Project,
        sink: DiagnosticSinkId::Project,
        file_name: "agent-project.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:policy"),
        owner: ModuleOwner::Policy,
        sink: DiagnosticSinkId::Policy,
        file_name: "agent-policy.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:diagnostic:audit"),
        owner: ModuleOwner::Audit,
        sink: DiagnosticSinkId::AuditDiagnostic,
        file_name: "audit-diagnostic.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:agent:update"),
        owner: ModuleOwner::Update,
        sink: DiagnosticSinkId::Update,
        file_name: "agent-update.log",
    },
    TargetSpec {
        target: LogTarget::new("aemeath:context"),
        owner: ModuleOwner::Context,
        sink: DiagnosticSinkId::Context,
        file_name: "context.log",
    },
];

pub(crate) const FALLBACK: TargetSpec = TargetSpec {
    target: LogTarget::new("aemeath"),
    owner: ModuleOwner::Shared,
    sink: DiagnosticSinkId::Fallback,
    file_name: "aemeath.log",
};
