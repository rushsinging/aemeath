use crate::application::constants::{FINGERPRINT_MAX_REPEAT, FINGERPRINT_WINDOW};

pub(crate) struct StallDetector {
    recent_fingerprints: Vec<String>,
    max_fingerprint_repeat: usize,
}

impl StallDetector {
    pub(crate) fn new() -> Self {
        Self {
            recent_fingerprints: Vec::new(),
            max_fingerprint_repeat: 0,
        }
    }

    pub(crate) fn record_text(&mut self, text: &str) -> bool {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            let fp: String = trimmed.chars().take(200).collect();
            self.recent_fingerprints.push(fp);
            if self.recent_fingerprints.len() > FINGERPRINT_WINDOW {
                self.recent_fingerprints.remove(0);
            }
        }

        if self.recent_fingerprints.len() < FINGERPRINT_MAX_REPEAT {
            return false;
        }

        let last = &self.recent_fingerprints[self.recent_fingerprints.len() - 1];
        let repeat_count = self
            .recent_fingerprints
            .iter()
            .rev()
            .take(FINGERPRINT_MAX_REPEAT)
            .filter(|fp| *fp == last)
            .count();
        if repeat_count > self.max_fingerprint_repeat {
            self.max_fingerprint_repeat = repeat_count;
            log::debug!(target: crate::LOG_TARGET,
                "[stall] fingerprint repeat count: {} (max so far: {})",
                repeat_count,
                self.max_fingerprint_repeat
            );
        }
        if repeat_count >= FINGERPRINT_MAX_REPEAT {
            log::warn!(target: crate::LOG_TARGET,
                "[stall] assistant text repeated {} times in recent {} run steps (max: {})",
                repeat_count,
                self.recent_fingerprints.len(),
                self.max_fingerprint_repeat
            );
            return true;
        }
        false
    }
}
