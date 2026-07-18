mod context;
mod routing;
#[cfg(test)]
#[path = "domain/routing_guard.rs"]
mod routing_guard;

pub use context::{FieldPatch, LogContext, LogContextPatch};
pub(crate) use routing::{DiagnosticSinkId, TargetCatalog};
