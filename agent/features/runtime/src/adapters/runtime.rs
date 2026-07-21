//! Port trait adapters for concrete production types.
//!
//! Runtime no longer adapts a concrete legacy hook executor: notifications are dispatched
//! through the injected HookPort at their production call sites.
