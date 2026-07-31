use crate::tui::model::conversation::interaction::UiRunId;
use std::time::{Duration, Instant};

const MODEL_SILENCE_THRESHOLD: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunActivityState {
    main_run_id: Option<UiRunId>,
    invoking_model_silence_started_at: Option<Instant>,
    silence_interval: u64,
    pub frame: u64,
    pub verb: String,
}

impl Default for RunActivityState {
    fn default() -> Self {
        Self {
            main_run_id: None,
            invoking_model_silence_started_at: None,
            silence_interval: 0,
            frame: 0,
            verb: String::new(),
        }
    }
}

impl RunActivityState {
    pub fn sync_main_run(&mut self, run_id: Option<&UiRunId>, invoking_model: bool, now: Instant) {
        let identity_changed = self.main_run_id.as_ref() != run_id;
        if identity_changed {
            self.main_run_id = run_id.cloned();
            self.frame = 0;
            self.verb.clear();
        }

        if run_id.is_some() && invoking_model {
            if identity_changed || self.invoking_model_silence_started_at.is_none() {
                self.begin_silence_interval(now);
            }
        } else {
            self.invoking_model_silence_started_at = None;
        }
    }

    pub fn observe_main_model_activity(&mut self, run_id: &UiRunId, now: Instant) -> bool {
        if self.main_run_id.as_ref() != Some(run_id)
            || self.invoking_model_silence_started_at.is_none()
        {
            return false;
        }
        self.begin_silence_interval(now);
        true
    }

    pub fn advance_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    pub fn is_active(&self) -> bool {
        self.main_run_id.is_some()
    }

    pub fn is_model_silent(&self, now: Instant) -> bool {
        self.invoking_model_silence_started_at
            .is_some_and(|started_at| {
                now.saturating_duration_since(started_at) >= MODEL_SILENCE_THRESHOLD
            })
    }

    pub fn elapsed_secs(&self, now: Instant) -> u64 {
        self.invoking_model_silence_started_at
            .map_or(0, |started_at| {
                now.saturating_duration_since(started_at).as_secs()
            })
    }

    pub fn silence_block_id(&self) -> Option<String> {
        let run_id = self.main_run_id.as_ref()?;
        self.invoking_model_silence_started_at?;
        Some(format!(
            "run-model-silence-{}-{}",
            run_id.as_str(),
            self.silence_interval
        ))
    }

    fn begin_silence_interval(&mut self, now: Instant) {
        self.invoking_model_silence_started_at = Some(now);
        self.silence_interval = self.silence_interval.wrapping_add(1);
    }
}
