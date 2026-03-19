use std::io::{self, IsTerminal, Write};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::NotificationSettings;

pub(crate) const LONG_WAIT_NOTIFICATION_THRESHOLD: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalNotification {
    Milestone(String),
    LongWaitFinished { label: String, elapsed: Duration },
    Failure(String),
}

impl TerminalNotification {
    fn cue_message(&self) -> String {
        match self {
            Self::Milestone(label) => format!("Major milestone reached: {label}"),
            Self::LongWaitFinished { label, elapsed } => format!(
                "Long wait finished after {}: {label}",
                format_duration(*elapsed)
            ),
            Self::Failure(label) => format!("Attention needed: {label}"),
        }
    }
}

trait BellEmitter {
    fn is_tty(&self) -> bool;
    fn ring(&mut self) -> io::Result<()>;
}

#[derive(Debug, Default)]
struct StderrBellEmitter;

impl BellEmitter for StderrBellEmitter {
    fn is_tty(&self) -> bool {
        io::stderr().is_terminal()
    }

    fn ring(&mut self) -> io::Result<()> {
        let mut stderr = io::stderr();
        stderr.write_all(b"\x07")?;
        stderr.flush()
    }
}

#[derive(Debug)]
pub(crate) struct TerminalNotifier {
    enabled: bool,
    emitter: StderrBellEmitter,
}

impl TerminalNotifier {
    pub(crate) fn new(settings: &NotificationSettings) -> Self {
        Self {
            enabled: settings.enabled,
            emitter: StderrBellEmitter,
        }
    }

    pub(crate) fn notify(&mut self, notification: TerminalNotification) -> Result<Option<String>> {
        notify_with_emitter(self.enabled, &mut self.emitter, notification)
    }

    pub(crate) fn notify_long_wait_finished(
        &mut self,
        label: impl Into<String>,
        elapsed: Duration,
    ) -> Result<Option<String>> {
        notify_long_wait_finished_with_emitter(
            self.enabled,
            &mut self.emitter,
            label.into(),
            elapsed,
        )
    }
}

fn notify_with_emitter<E>(
    enabled: bool,
    emitter: &mut E,
    notification: TerminalNotification,
) -> Result<Option<String>>
where
    E: BellEmitter,
{
    if !enabled {
        return Ok(None);
    }

    let cue = notification.cue_message();
    if emitter.is_tty() {
        emitter
            .ring()
            .context("failed to emit the terminal notification bell")?;
    }
    Ok(Some(cue))
}

fn notify_long_wait_finished_with_emitter<E>(
    enabled: bool,
    emitter: &mut E,
    label: String,
    elapsed: Duration,
) -> Result<Option<String>>
where
    E: BellEmitter,
{
    if !enabled || elapsed < LONG_WAIT_NOTIFICATION_THRESHOLD {
        return Ok(None);
    }

    notify_with_emitter(
        enabled,
        emitter,
        TerminalNotification::LongWaitFinished { label, elapsed },
    )
}

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes == 0 {
        format!("{seconds}s")
    } else {
        format!("{minutes}m {seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct TestBellEmitter {
        is_tty: bool,
        rings: usize,
    }

    impl BellEmitter for TestBellEmitter {
        fn is_tty(&self) -> bool {
            self.is_tty
        }

        fn ring(&mut self) -> io::Result<()> {
            self.rings += 1;
            Ok(())
        }
    }

    #[test]
    fn notifier_skips_disabled_notifications() {
        let mut emitter = TestBellEmitter {
            is_tty: true,
            rings: 0,
        };

        let cue = notify_with_emitter(
            false,
            &mut emitter,
            TerminalNotification::Milestone("Validation passed".to_string()),
        )
        .expect("disabled notifier should not fail");

        assert_eq!(cue, None);
        assert_eq!(emitter.rings, 0);
    }

    #[test]
    fn notifier_rings_bell_when_tty_is_available() {
        let mut emitter = TestBellEmitter {
            is_tty: true,
            rings: 0,
        };

        let cue = notify_with_emitter(
            true,
            &mut emitter,
            TerminalNotification::Milestone("Validation passed".to_string()),
        )
        .expect("tty cue should succeed");

        assert_eq!(
            cue.as_deref(),
            Some("Major milestone reached: Validation passed")
        );
        assert_eq!(emitter.rings, 1);
    }

    #[test]
    fn notifier_falls_back_to_text_only_when_tty_is_unavailable() {
        let mut emitter = TestBellEmitter {
            is_tty: false,
            rings: 0,
        };

        let cue = notify_with_emitter(
            true,
            &mut emitter,
            TerminalNotification::Milestone("Validation passed".to_string()),
        )
        .expect("non-tty cue should succeed");

        assert_eq!(
            cue.as_deref(),
            Some("Major milestone reached: Validation passed")
        );
        assert_eq!(emitter.rings, 0);
    }

    #[test]
    fn notifier_only_emits_long_wait_cues_after_threshold() {
        let mut emitter = TestBellEmitter {
            is_tty: true,
            rings: 0,
        };

        let cue = notify_long_wait_finished_with_emitter(
            true,
            &mut emitter,
            "Scan agent finished".to_string(),
            Duration::from_secs(9),
        )
        .expect("below-threshold wait should succeed");
        assert_eq!(cue, None);
        assert_eq!(emitter.rings, 0);

        let cue = notify_long_wait_finished_with_emitter(
            true,
            &mut emitter,
            "Scan agent finished".to_string(),
            Duration::from_secs(11),
        )
        .expect("long wait cue should succeed");
        assert_eq!(
            cue.as_deref(),
            Some("Long wait finished after 11s: Scan agent finished")
        );
        assert_eq!(emitter.rings, 1);
    }
}
