use std::sync::Arc;

use serde::{Deserialize, Serialize};
use storage::api::{
    AtomicBlobPort, DeleteOptions, Durability, Generation, ReadOutcome, SafePathSegment,
    StorageKey, StorageNamespace, WriteOptions,
};

use crate::domain::session::{AcceptedInputProjection, CanonicalSession};

const ACCEPTED_INPUT_LEDGER_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedAcceptedInput {
    revision: u64,
    run_id: String,
    step_id: String,
    input: AcceptedInputProjection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AcceptedInputLedger {
    schema_version: u32,
    session_id: String,
    inputs: Vec<PersistedAcceptedInput>,
}

impl AcceptedInputLedger {
    fn empty(session_id: &str) -> Self {
        Self {
            schema_version: ACCEPTED_INPUT_LEDGER_SCHEMA_VERSION,
            session_id: session_id.to_string(),
            inputs: Vec::new(),
        }
    }
}

pub(crate) struct AtomicBlobAcceptedInputLedger {
    blob: Arc<dyn AtomicBlobPort>,
    key: StorageKey,
    session_id: String,
}

impl AtomicBlobAcceptedInputLedger {
    pub(crate) fn new(blob: Arc<dyn AtomicBlobPort>, session_id: &str) -> Result<Self, String> {
        let segment = format!("accepted-input-ledger-{session_id}")
            .parse::<SafePathSegment>()
            .map_err(|error| error.to_string())?;
        let key = StorageKey::new(StorageNamespace::Session, vec![segment])
            .map_err(|error| error.to_string())?;
        Ok(Self {
            blob,
            key,
            session_id: session_id.to_string(),
        })
    }

    async fn read(&self) -> Result<AcceptedInputLedger, String> {
        let ledger = match self
            .blob
            .read(&self.key, Generation::Primary)
            .await
            .map_err(|error| error.to_string())?
        {
            ReadOutcome::Found(read) => serde_json::from_slice::<AcceptedInputLedger>(read.bytes())
                .map_err(|error| error.to_string())?,
            ReadOutcome::NotFound => AcceptedInputLedger::empty(&self.session_id),
        };
        if ledger.schema_version != ACCEPTED_INPUT_LEDGER_SCHEMA_VERSION {
            return Err(format!(
                "accepted input ledger schema version 不受支持：{}",
                ledger.schema_version
            ));
        }
        if ledger.session_id != self.session_id {
            return Err("accepted input ledger Session identity 不匹配".to_string());
        }
        Ok(ledger)
    }

    pub(crate) async fn save(
        &self,
        revision: u64,
        run_id: &str,
        step_id: &str,
        input: &AcceptedInputProjection,
    ) -> Result<(), String> {
        let mut ledger = self.read().await?;
        if let Some(existing) = ledger
            .inputs
            .iter_mut()
            .find(|entry| entry.run_id == run_id && entry.step_id == step_id)
        {
            if existing.input.fingerprint != input.fingerprint {
                return Err("accepted input ledger 内容冲突".to_string());
            }
            if revision < existing.revision {
                return Err("accepted input ledger revision 回退".to_string());
            }
            existing.revision = revision;
            existing.input = input.clone();
        } else {
            ledger.inputs.push(PersistedAcceptedInput {
                revision,
                run_id: run_id.to_string(),
                step_id: step_id.to_string(),
                input: input.clone(),
            });
        }
        let bytes = serde_json::to_vec(&ledger).map_err(|error| error.to_string())?;
        self.blob
            .write_atomic(
                &self.key,
                &bytes,
                WriteOptions::new(Durability::ProcessCrashSafe),
            )
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) async fn overlay(&self, session: &mut CanonicalSession) -> Result<(), String> {
        let ledger = self.read().await?;
        for entry in ledger.inputs {
            session.append_accepted_input(&entry.run_id, &entry.step_id, entry.input);
        }
        Ok(())
    }

    pub(crate) async fn acknowledge_finalized_input(
        &self,
        run_id: &str,
        step_id: &str,
    ) -> Result<(), String> {
        let mut ledger = self.read().await?;
        ledger
            .inputs
            .retain(|entry| entry.run_id != run_id || entry.step_id != step_id);
        if ledger.inputs.is_empty() {
            return self.delete().await;
        }
        let bytes = serde_json::to_vec(&ledger).map_err(|error| error.to_string())?;
        self.blob
            .write_atomic(
                &self.key,
                &bytes,
                WriteOptions::new(Durability::ProcessCrashSafe),
            )
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    pub(crate) async fn delete(&self) -> Result<(), String> {
        self.blob
            .delete_all_generations(&self.key, DeleteOptions::default())
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "accepted_input_ledger_tests.rs"]
mod tests;
