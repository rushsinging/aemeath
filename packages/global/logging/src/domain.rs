mod routing;
#[cfg(test)]
#[path = "domain/routing_guard.rs"]
mod routing_guard;

pub(crate) use routing::{DiagnosticSinkId, TargetCatalog};
