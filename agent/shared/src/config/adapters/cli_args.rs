//! CliArgsAdapter — translates CLI arguments into a ConfigPatch.
//!
//! After removing clap `env=` attributes (D3), CLI args only carry
//! values explicitly passed on the command line. Env vars are handled
//! exclusively by `EnvAdapter`.

// TODO: S1 — implement from_args() once ChatBootstrapArgs is available.
// For now this is a placeholder; full implementation in S2 when
// consumer migration happens.

use crate::config::domain::merge::ConfigPatch;

/// Placeholder — will be populated from clap-parsed args in S2.
pub fn read() -> ConfigPatch {
    ConfigPatch::default()
}
