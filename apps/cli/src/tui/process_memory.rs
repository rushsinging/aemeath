use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProcessMemorySnapshot {
    pub(crate) current_rss_bytes: u64,
    pub(crate) peak_rss_bytes: u64,
    pub(crate) first_rss_bytes: u64,
    pub(crate) growth_from_first_bytes: i64,
    pub(crate) growth_from_previous_bytes: i64,
}

pub(crate) struct ProcessMemoryBaseline {
    sample_interval: Duration,
    last_attempt_at: Option<Duration>,
    last_snapshot: Option<ProcessMemorySnapshot>,
    first_rss_bytes: Option<u64>,
    previous_rss_bytes: Option<u64>,
    peak_rss_bytes: u64,
}

impl ProcessMemoryBaseline {
    pub(crate) fn new(sample_interval: Duration) -> Self {
        Self {
            sample_interval,
            last_attempt_at: None,
            last_snapshot: None,
            first_rss_bytes: None,
            previous_rss_bytes: None,
            peak_rss_bytes: 0,
        }
    }

    pub(crate) fn observe(
        &mut self,
        now: Duration,
        current_rss_bytes: Option<u64>,
    ) -> Option<ProcessMemorySnapshot> {
        if self
            .last_attempt_at
            .is_some_and(|last| now.saturating_sub(last) < self.sample_interval)
        {
            return self.last_snapshot;
        }
        self.last_attempt_at = Some(now);
        let current_rss_bytes = current_rss_bytes?;
        let first_rss_bytes = *self.first_rss_bytes.get_or_insert(current_rss_bytes);
        let previous_rss_bytes = self.previous_rss_bytes.unwrap_or(current_rss_bytes);
        self.previous_rss_bytes = Some(current_rss_bytes);
        self.peak_rss_bytes = self.peak_rss_bytes.max(current_rss_bytes);
        let snapshot = ProcessMemorySnapshot {
            current_rss_bytes,
            peak_rss_bytes: self.peak_rss_bytes,
            first_rss_bytes,
            growth_from_first_bytes: signed_delta(current_rss_bytes, first_rss_bytes),
            growth_from_previous_bytes: signed_delta(current_rss_bytes, previous_rss_bytes),
        };
        self.last_snapshot = Some(snapshot);
        Some(snapshot)
    }
}

fn signed_delta(current: u64, baseline: u64) -> i64 {
    let delta = i128::from(current) - i128::from(baseline);
    i64::try_from(delta).unwrap_or_else(|_| {
        if delta.is_negative() {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn current_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = u64::try_from(page_size).ok()?;
    resident_pages.checked_mul(page_size)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
pub(crate) fn current_rss_bytes() -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::mach_task_basic_info>::uninit();
    let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
    let result = unsafe {
        libc::task_info(
            libc::mach_task_self(),
            libc::MACH_TASK_BASIC_INFO,
            info.as_mut_ptr().cast::<libc::integer_t>(),
            &mut count,
        )
    };
    if result != libc::KERN_SUCCESS {
        return None;
    }
    let info = unsafe { info.assume_init() };
    Some(info.resident_size)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn current_rss_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
#[path = "process_memory_tests.rs"]
mod tests;
