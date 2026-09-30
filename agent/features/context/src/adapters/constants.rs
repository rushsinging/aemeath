//! Context adapters 层共享生产常量（#1146 双轨归位）。

pub(crate) const ACCEPTED_INPUT_LEDGER_SCHEMA_VERSION: u32 = 1;
pub(crate) const RECEIPT_LEDGER_SCHEMA_VERSION: u32 = 1;
pub(crate) const INJECTION_CANDIDATE_LIMIT: usize = 200;
