pub(crate) const NATIVE_STDERR_FILE: &str = "native-stderr.log";

use super::routing::{DiagnosticSinkId, LogTarget, ModuleOwner, TargetSpec};

pub(crate) const FALLBACK: TargetSpec = TargetSpec {
    target: LogTarget::new("aemeath"),
    owner: ModuleOwner::Shared,
    sink: DiagnosticSinkId::Fallback,
    file_name: "aemeath.log",
};
