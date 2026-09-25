pub mod coverage;
pub mod guard_registry;
pub mod guards;
pub mod guards_engine;
pub mod guards_facade_trim;
pub mod guards_rules;
pub mod reachability;
pub mod sdk_wire_schema;
pub mod source_guard;
pub mod workspace_guard;

#[cfg(test)]
#[path = "sdk_wire_schema_tests.rs"]
mod sdk_wire_schema_tests;

#[cfg(test)]
#[path = "guard_registry_tests.rs"]
mod guard_registry_tests;

#[cfg(test)]
#[path = "guards_engine_tests.rs"]
mod guards_engine_tests;

#[cfg(test)]
#[path = "guards_rules_tests.rs"]
mod guards_rules_tests;

#[cfg(test)]
#[path = "guards_tests.rs"]
mod guards_tests;
