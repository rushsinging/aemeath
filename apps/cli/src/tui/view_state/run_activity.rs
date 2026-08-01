use crate::tui::model::conversation::interaction::UiRunId;
use std::time::{Duration, Instant};

const MODEL_SILENCE_THRESHOLD: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunActivityState {
    main_run_id: Option<UiRunId>,
    invoking_model_silence_started_at: Option<Instant>,
    timing_observed_at: Option<Instant>,
    timing_observation_revision: Option<u64>,
    total_elapsed_ms: u64,
    phase_elapsed_ms: u64,
    silence_interval: u64,
    pub frame: u64,
    pub verb: String,
}

impl RunActivityState {
    pub fn sync_activity_summary(
        &mut self,
        summary: Option<&crate::tui::view_assembler::activity_summary::ActivitySummary>,
        now: Instant,
    ) {
        let Some(summary) = summary else {
            self.sync_main_run(None, false, 0, 0, 0, now);
            return;
        };
        self.sync_main_run(
            Some(&summary.run_id),
            summary.invoking_model,
            summary.revision,
            summary.total_elapsed_ms,
            summary.phase_elapsed_ms,
            now,
        );
    }

    pub fn sync_main_run(
        &mut self,
        run_id: Option<&UiRunId>,
        invoking_model: bool,
        timing_observation_revision: u64,
        total_elapsed_ms: u64,
        phase_elapsed_ms: u64,
        now: Instant,
    ) {
        let identity_changed = self.main_run_id.as_ref() != run_id;
        if identity_changed {
            self.main_run_id = run_id.cloned();
            self.timing_observation_revision = None;
            self.frame = 0;
            self.verb.clear();
        }
        let timing_observation_changed = run_id.is_some()
            && self.timing_observation_revision != Some(timing_observation_revision);
        if timing_observation_changed {
            self.timing_observation_revision = Some(timing_observation_revision);
            self.total_elapsed_ms = total_elapsed_ms;
            self.phase_elapsed_ms = phase_elapsed_ms;
            self.timing_observed_at = Some(now);
        } else if run_id.is_none() {
            self.timing_observation_revision = None;
            self.total_elapsed_ms = 0;
            self.phase_elapsed_ms = 0;
            self.timing_observed_at = None;
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

    pub fn total_elapsed_secs(&self, now: Instant) -> u64 {
        self.elapsed_ms_since_observation(now, self.total_elapsed_ms) / 1000
    }

    pub fn phase_elapsed_secs(&self, now: Instant) -> u64 {
        self.elapsed_ms_since_observation(now, self.phase_elapsed_ms) / 1000
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

    fn elapsed_ms_since_observation(&self, now: Instant, baseline_ms: u64) -> u64 {
        let delta_ms = self.timing_observed_at.map_or(0, |observed_at| {
            u64::try_from(now.saturating_duration_since(observed_at).as_millis())
                .unwrap_or(u64::MAX)
        });
        baseline_ms.saturating_add(delta_ms)
    }

    fn begin_silence_interval(&mut self, now: Instant) {
        self.invoking_model_silence_started_at = Some(now);
        self.silence_interval = self.silence_interval.wrapping_add(1);
    }
}
