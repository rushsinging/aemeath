use crate::tui::model::conversation::interaction::UiRunId;
use std::time::{Duration, Instant};

const MODEL_SILENCE_THRESHOLD: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunActivityState {
    main_run_id: Option<UiRunId>,
    invoking_model_silence_started_at: Option<Instant>,
    total_timing_observed_at: Option<Instant>,
    phase_timing_observed_at: Option<Instant>,
    root_timing_revision: Option<u64>,
    phase_timing_revision: Option<u64>,
    root_activity_id: Option<String>,
    primary_activity_id: Option<String>,
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
            self.sync_main_run(None, false, 0, 0, 0, 0, now);
            return;
        };
        let primary = summary.primary.as_ref();
        self.sync_main_run_with_activity_ids(
            Some(&summary.run_id),
            primary.is_some_and(|primary| primary.invoking_model),
            Some(&summary.root_activity_id),
            summary.root_timing_revision,
            summary.total_elapsed_ms,
            primary.map(|primary| primary.activity_id.as_str()),
            primary.map_or(0, |primary| primary.timing_revision),
            primary.map_or(0, |primary| primary.elapsed_ms),
            now,
        );
    }

    pub fn sync_main_run(
        &mut self,
        run_id: Option<&UiRunId>,
        invoking_model: bool,
        root_timing_revision: u64,
        total_elapsed_ms: u64,
        phase_timing_revision: u64,
        phase_elapsed_ms: u64,
        now: Instant,
    ) {
        self.sync_main_run_with_activity_ids(
            run_id,
            invoking_model,
            Some("__compat_root__"),
            root_timing_revision,
            total_elapsed_ms,
            run_id.map(|_| "__compat_primary__"),
            phase_timing_revision,
            phase_elapsed_ms,
            now,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn sync_main_run_with_activity_ids(
        &mut self,
        run_id: Option<&UiRunId>,
        invoking_model: bool,
        root_activity_id: Option<&str>,
        root_timing_revision: u64,
        total_elapsed_ms: u64,
        primary_activity_id: Option<&str>,
        phase_timing_revision: u64,
        phase_elapsed_ms: u64,
        now: Instant,
    ) {
        let identity_changed = self.main_run_id.as_ref() != run_id;
        if identity_changed {
            self.main_run_id = run_id.cloned();
            self.root_timing_revision = None;
            self.phase_timing_revision = None;
            self.root_activity_id = None;
            self.primary_activity_id = None;
            self.frame = 0;
            self.verb.clear();
        }
        let root_timing_changed =
            run_id.is_some() && self.root_timing_revision != Some(root_timing_revision);
        if root_timing_changed {
            self.root_activity_id = root_activity_id.map(str::to_string);
            self.root_timing_revision = Some(root_timing_revision);
            self.total_elapsed_ms = total_elapsed_ms;
            self.total_timing_observed_at = Some(now);
        }
        let phase_timing_changed = run_id.is_some()
            && primary_activity_id.is_some()
            && self.phase_timing_revision != Some(phase_timing_revision);
        if phase_timing_changed {
            self.primary_activity_id = primary_activity_id.map(str::to_string);
            self.phase_timing_revision = Some(phase_timing_revision);
            self.phase_elapsed_ms = phase_elapsed_ms;
            self.phase_timing_observed_at = Some(now);
        } else if run_id.is_some() && primary_activity_id.is_none() {
            self.primary_activity_id = None;
            self.phase_timing_revision = None;
            self.phase_elapsed_ms = 0;
            self.phase_timing_observed_at = None;
        }
        if run_id.is_none() {
            self.root_timing_revision = None;
            self.phase_timing_revision = None;
            self.root_activity_id = None;
            self.primary_activity_id = None;
            self.total_elapsed_ms = 0;
            self.phase_elapsed_ms = 0;
            self.total_timing_observed_at = None;
            self.phase_timing_observed_at = None;
        }

        if identity_changed || root_timing_changed || phase_timing_changed || run_id.is_none() {
            crate::tui::log_debug!(
                "[ACTIVITY_TIMING] state_sync run_id={} identity_changed={} root_activity_id={} root_revision={} root_changed={} total_elapsed_ms={} primary_activity_id={} phase_revision={} phase_changed={} phase_elapsed_ms={} cleared={}",
                run_id.map_or("-", UiRunId::as_str),
                identity_changed,
                root_activity_id.unwrap_or("-"),
                root_timing_revision,
                root_timing_changed,
                total_elapsed_ms,
                primary_activity_id.unwrap_or("-"),
                phase_timing_revision,
                phase_timing_changed,
                phase_elapsed_ms,
                run_id.is_none(),
            );
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

    pub(crate) fn root_timing_identity(&self) -> Option<(&str, u64)> {
        Some((
            self.root_activity_id.as_deref()?,
            self.root_timing_revision?,
        ))
    }

    pub(crate) fn phase_timing_identity(&self) -> Option<(&str, u64)> {
        Some((
            self.primary_activity_id.as_deref()?,
            self.phase_timing_revision?,
        ))
    }

    pub fn total_elapsed_secs(&self, now: Instant) -> u64 {
        self.elapsed_ms_since_observation(now, self.total_elapsed_ms, self.total_timing_observed_at)
            / 1000
    }

    pub fn phase_elapsed_secs(&self, now: Instant) -> u64 {
        self.elapsed_ms_since_observation(now, self.phase_elapsed_ms, self.phase_timing_observed_at)
            / 1000
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

    fn elapsed_ms_since_observation(
        &self,
        now: Instant,
        baseline_ms: u64,
        observed_at: Option<Instant>,
    ) -> u64 {
        let delta_ms = observed_at.map_or(0, |observed_at| {
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
