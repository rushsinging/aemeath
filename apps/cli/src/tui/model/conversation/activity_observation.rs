use crate::tui::adapter::tui_runtime_event::{
    TuiActivityObservation, TuiActivitySnapshot, UiActivityId,
};
use crate::tui::model::conversation::interaction::UiRunId;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ActivityObservationModel {
    revision_by_run: Vec<(UiRunId, u64)>,
    stale_runs: Vec<UiRunId>,
    activities: Vec<TuiActivityObservation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActivityIncrementOutcome {
    Applied,
    Ignored,
    GapDetected,
}

impl ActivityObservationModel {
    pub(crate) fn activities(&self) -> &[TuiActivityObservation] {
        &self.activities
    }

    pub(crate) fn revision_for(&self, run_id: &UiRunId) -> Option<u64> {
        self.revision_by_run
            .iter()
            .find_map(|(stored_run_id, revision)| (stored_run_id == run_id).then_some(*revision))
    }

    pub(crate) fn is_stale(&self, run_id: &UiRunId) -> bool {
        self.stale_runs
            .iter()
            .any(|stale_run_id| stale_run_id == run_id)
    }

    #[cfg(test)]
    pub(crate) fn replace_for_test(
        &mut self,
        run_id: UiRunId,
        revision: u64,
        activities: Vec<TuiActivityObservation>,
    ) {
        self.activities.retain(|activity| activity.run_id != run_id);
        self.activities.extend(activities);
        self.set_revision(run_id.clone(), revision);
        self.clear_stale(&run_id);
    }

    pub(crate) fn observe_increment(
        &mut self,
        activity: TuiActivityObservation,
    ) -> ActivityIncrementOutcome {
        let run_id = activity.run_id.clone();
        let current_revision = self.revision_for(&run_id).unwrap_or(0);
        if activity.revision <= current_revision {
            return ActivityIncrementOutcome::Ignored;
        }
        if activity.revision != current_revision.saturating_add(1) {
            self.mark_stale(run_id);
            return ActivityIncrementOutcome::GapDetected;
        }

        self.upsert_activity(activity);
        self.set_revision(run_id.clone(), current_revision.saturating_add(1));
        self.clear_stale(&run_id);
        ActivityIncrementOutcome::Applied
    }

    pub(crate) fn replace_snapshot(&mut self, snapshot: TuiActivitySnapshot) -> bool {
        let current_revision = self.revision_for(&snapshot.run_id).unwrap_or(0);
        if snapshot.revision < current_revision
            || snapshot.activities.iter().any(|activity| {
                activity.run_id != snapshot.run_id || activity.revision > snapshot.revision
            })
        {
            return false;
        }
        if snapshot.revision == current_revision && !self.is_stale(&snapshot.run_id) {
            return false;
        }

        let run_id = snapshot.run_id;
        self.activities.retain(|activity| activity.run_id != run_id);
        self.activities.extend(snapshot.activities);
        self.set_revision(run_id.clone(), snapshot.revision);
        self.clear_stale(&run_id);
        true
    }

    fn upsert_activity(&mut self, activity: TuiActivityObservation) {
        if let Some(stored) = self
            .activities
            .iter_mut()
            .find(|stored| stored.run_id == activity.run_id && stored.id == activity.id)
        {
            *stored = activity;
        } else {
            self.activities.push(activity);
        }
    }

    fn set_revision(&mut self, run_id: UiRunId, revision: u64) {
        if let Some((_, stored_revision)) = self
            .revision_by_run
            .iter_mut()
            .find(|(stored_run_id, _)| stored_run_id == &run_id)
        {
            *stored_revision = revision;
        } else {
            self.revision_by_run.push((run_id, revision));
        }
    }

    fn mark_stale(&mut self, run_id: UiRunId) {
        if !self.stale_runs.iter().any(|stored| stored == &run_id) {
            self.stale_runs.push(run_id);
        }
    }

    fn clear_stale(&mut self, run_id: &UiRunId) {
        self.stale_runs.retain(|stored| stored != run_id);
    }

    #[allow(dead_code)]
    pub(crate) fn activity(&self, id: &UiActivityId) -> Option<&TuiActivityObservation> {
        self.activities.iter().find(|activity| &activity.id == id)
    }
}
