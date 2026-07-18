use crate::LOG_TARGET;

pub(crate) struct StallDetector {
    recent_fingerprints: Vec<String>,
    max_fingerprint_repeat: usize,
}

impl StallDetector {
    const FINGERPRINT_WINDOW: usize = 4;
    const FINGERPRINT_MAX_REPEAT: usize = 3;

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
            if self.recent_fingerprints.len() > Self::FINGERPRINT_WINDOW {
                self.recent_fingerprints.remove(0);
            }
        }

        if self.recent_fingerprints.len() < Self::FINGERPRINT_MAX_REPEAT {
            return false;
        }

        let last = &self.recent_fingerprints[self.recent_fingerprints.len() - 1];
        let repeat_count = self
            .recent_fingerprints
            .iter()
            .rev()
            .take(Self::FINGERPRINT_MAX_REPEAT)
            .filter(|fp| *fp == last)
            .count();
        if repeat_count > self.max_fingerprint_repeat {
            self.max_fingerprint_repeat = repeat_count;
            log::debug!(target: LOG_TARGET,
                "[stall] fingerprint repeat count: {} (max so far: {})",
                repeat_count,
                self.max_fingerprint_repeat
            );
        }
        if repeat_count >= Self::FINGERPRINT_MAX_REPEAT {
            log::warn!(target: LOG_TARGET,
                "[stall] assistant text repeated {} times in recent {} turns (max: {})",
                repeat_count,
                self.recent_fingerprints.len(),
                self.max_fingerprint_repeat
            );
            return true;
        }
        false
    }
}
