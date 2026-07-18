//! Config adapter layer — translates external formats into ConfigPatch.
//!
//! Each adapter is responsible for ONE source:
//! - `cli_args`: CLI arguments (from clap) — pure fn
//! - `file`: JSON config files — stub, fs IO moves to runtime feature
//! - `claude`: Claude Code settings.json ACL — stub, fs IO moves to runtime feature

pub mod claude;
pub mod cli_args;
pub mod file;
pub mod paths;
