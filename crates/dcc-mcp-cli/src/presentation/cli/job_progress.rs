use std::time::{Duration, Instant};

use crate::application::control_plane::JobWaitProgress;

const PROGRESS_BAR_WIDTH: u64 = 20;
const PROGRESS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct JobProgressReporter {
    last_status: Option<String>,
    last_bucket: Option<u64>,
    last_emit_at: Option<Instant>,
}

impl JobProgressReporter {
    pub(super) fn next_line(&mut self, update: &JobWaitProgress, now: Instant) -> Option<String> {
        let status_changed = self.last_status.as_deref() != Some(&update.status);
        self.last_status = Some(update.status.clone());
        let heartbeat_due = self
            .last_emit_at
            .is_none_or(|last| now.saturating_duration_since(last) >= PROGRESS_HEARTBEAT_INTERVAL);

        if let (Some(current), Some(total)) = (update.current, update.total)
            && total > 0
        {
            let current = current.min(total);
            let filled = ((current as u128 * PROGRESS_BAR_WIDTH as u128) / total as u128) as u64;
            if !status_changed && self.last_bucket == Some(filled) && !heartbeat_due {
                return None;
            }
            self.last_bucket = Some(filled);
            self.last_emit_at = Some(now);
            let percent = ((current as u128 * 100) / total as u128) as u64;
            let bar = format!(
                "{}{}",
                "#".repeat(filled as usize),
                "-".repeat((PROGRESS_BAR_WIDTH - filled) as usize)
            );
            return Some(with_progress_message(
                format!(
                    "progress {} [{bar}] {percent:>3}% ({current}/{total}) {}",
                    update.job_id, update.status
                ),
                update.message.as_deref(),
            ));
        }

        if !status_changed && !heartbeat_due {
            return None;
        }
        self.last_emit_at = Some(now);
        let current = update
            .current
            .map(|current| format!(" ({current} complete)"))
            .unwrap_or_default();
        Some(with_progress_message(
            format!("progress {} {}{current}", update.job_id, update.status),
            update.message.as_deref(),
        ))
    }
}

fn with_progress_message(mut line: String, message: Option<&str>) -> String {
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        line.push_str(": ");
        line.push_str(message);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_five_percent_steps_and_stalled_heartbeat() {
        let started = Instant::now();
        let mut reporter = JobProgressReporter::default();
        let mut update = JobWaitProgress {
            job_id: "job-42".to_string(),
            status: "running".to_string(),
            current: Some(0),
            total: Some(100),
            message: Some("starting".to_string()),
        };

        assert!(
            reporter.next_line(&update, started).is_some_and(|line| line
                .contains("[--------------------]")
                && line.contains("0/100"))
        );
        update.current = Some(4);
        assert!(reporter.next_line(&update, started).is_none());
        assert!(
            reporter
                .next_line(&update, started + PROGRESS_HEARTBEAT_INTERVAL)
                .is_some()
        );
        update.current = Some(5);
        assert!(
            reporter
                .next_line(&update, started + PROGRESS_HEARTBEAT_INTERVAL)
                .is_some_and(|line| line.contains("[#-------------------]") && line.contains("5%"))
        );
        update.status = "completed".to_string();
        assert!(
            reporter
                .next_line(&update, started + PROGRESS_HEARTBEAT_INTERVAL)
                .is_some()
        );
    }
}
