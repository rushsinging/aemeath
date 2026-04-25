use indicatif::{ProgressBar, ProgressStyle};
use std::time::{Duration, Instant};

pub struct ThinkingIndicator {
    progress: ProgressBar,
    start_time: Instant,
}

impl ThinkingIndicator {
    pub fn start(message: &str) -> Self {
        let progress = ProgressBar::new_spinner();
        progress.enable_steady_tick(Duration::from_millis(100));
        
        let style = ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg:.white}")
            .unwrap();
        
        progress.set_style(style);
        progress.set_message(message.to_string());
        
        Self {
            progress,
            start_time: Instant::now(),
        }
    }

    /// Return elapsed time since start, for t/s calculation.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn stop(self) {
        self.progress.finish_and_clear();
    }
}
