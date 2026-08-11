use std::sync::Arc;

use serde::{Deserialize, Serialize};
use storage::api::{
    AtomicBlobPort, DeleteOptions, Durability, Generation, ReadOutcome, SafePathSegment,
    StorageKey, StorageNamespace, WriteOptions,
};

use crate::domain::session::CanonicalSession;
use crate::domain::ToolCallReceipt;

const RECEIPT_LEDGER_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedToolReceipt {
    revision: u64,
    receipt: ToolCallReceipt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ToolReceiptLedger {
    schema_version: u32,
    session_id: String,
    receipts: Vec<PersistedToolReceipt>,
}

impl ToolReceiptLedger {
    fn empty(session_id: &str) -> Self {
        Self {
            schema_version: RECEIPT_LEDGER_SCHEMA_VERSION,
            session_id: session_id.to_string(),
            receipts: Vec::new(),
        }
    }
}

pub(crate) struct AtomicBlobToolReceiptLedger {
    blob: Arc<dyn AtomicBlobPort>,
    key: StorageKey,
    session_id: String,
}

impl AtomicBlobToolReceiptLedger {
    pub(crate) fn new(blob: Arc<dyn AtomicBlobPort>, session_id: &str) -> Result<Self, String> {
        let segment = format!("receipt-ledger-{session_id}")
            .parse::<SafePathSegment>()
            .map_err(|error| error.to_string())?;
        let key = StorageKey::new(StorageNamespace::ToolResult, vec![segment])
            .map_err(|error| error.to_string())?;
        Ok(Self {
            blob,
            key,
            session_id: session_id.to_string(),
        })
    }

    async fn read(&self) -> Result<ToolReceiptLedger, String> {
        let ledger = match self
            .blob
            .read(&self.key, Generation::Primary)
            .await
            .map_err(|error| error.to_string())?
        {
            ReadOutcome::Found(read) => serde_json::from_slice::<ToolReceiptLedger>(read.bytes())
                .map_err(|error| error.to_string())?,
            ReadOutcome::NotFound => ToolReceiptLedger::empty(&self.session_id),
        };
        if ledger.schema_version != RECEIPT_LEDGER_SCHEMA_VERSION {
            return Err(format!(
                "不支持的 Tool receipt ledger schema version：{}",
                ledger.schema_version
            ));
        }
        if ledger.session_id != self.session_id {
            return Err("Tool receipt ledger Session identity 不匹配".to_string());
        }
        Ok(ledger)
    }

    pub(crate) async fn save(
        &self,
        revision: u64,
        receipt: &ToolCallReceipt,
    ) -> Result<(), String> {
        let mut ledger = self.read().await?;
        if let Some(existing) = ledger
            .receipts
            .iter_mut()
            .find(|entry| entry.receipt.identity == receipt.identity)
        {
            if revision < existing.revision {
                return Err("Tool receipt ledger revision 回退".to_string());
            }
            existing.revision = revision;
            existing.receipt = receipt.clone();
        } else {
            ledger.receipts.push(PersistedToolReceipt {
                revision,
                receipt: receipt.clone(),
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
        for entry in ledger.receipts {
            session
                .advance_tool_receipt(crate::domain::ToolReceiptMutation {
                    identity: entry.receipt.identity.clone(),
                    input_preview: Some(entry.receipt.input_preview.clone()),
                    next: entry.receipt.state.clone(),
                })
                .map_err(|error| error.to_string())?;
        }
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
#[path = "tool_receipt_ledger_tests.rs"]
mod tests;
