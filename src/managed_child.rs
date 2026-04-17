use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};

const STREAM_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub(crate) enum ManagedChildOutput {
    Capture,
    File(File),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ManagedChildSettings {
    pub timeout: Duration,
    pub graceful_shutdown: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagedChildTermination {
    GracefulTerminated,
    ForceKilled,
}

impl ManagedChildTermination {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::GracefulTerminated => "sigterm",
            Self::ForceKilled => "sigkill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ManagedChildTimeout {
    pub pid: u32,
    pub timeout: Duration,
    pub elapsed: Duration,
    pub graceful_shutdown: Duration,
    pub termination: ManagedChildTermination,
}

impl ManagedChildTimeout {
    pub(crate) fn elapsed_seconds(self) -> u64 {
        self.elapsed.as_secs()
    }

    pub(crate) fn timeout_seconds(self) -> u64 {
        self.timeout.as_secs()
    }
}

#[derive(Debug)]
pub(crate) struct ManagedChildResult {
    pub status: ExitStatus,
    pub elapsed: Duration,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub timeout: Option<ManagedChildTimeout>,
}

#[derive(Debug)]
pub(crate) struct ManagedChild {
    child: Child,
    settings: ManagedChildSettings,
    started_at: Instant,
    capture_stdout: bool,
    capture_stderr: bool,
}

impl ManagedChild {
    /// Spawn a managed child process with subprocess-group supervision.
    ///
    /// Returns an error when the command cannot be configured or launched.
    pub(crate) fn spawn(
        command: &mut Command,
        stdout: ManagedChildOutput,
        stderr: ManagedChildOutput,
        settings: ManagedChildSettings,
    ) -> Result<Self> {
        configure_command_process_group(command)?;

        let capture_stdout = matches!(stdout, ManagedChildOutput::Capture);
        let capture_stderr = matches!(stderr, ManagedChildOutput::Capture);

        command.stdout(match stdout {
            ManagedChildOutput::Capture => Stdio::piped(),
            ManagedChildOutput::File(file) => Stdio::from(file),
        });
        command.stderr(match stderr {
            ManagedChildOutput::Capture => Stdio::piped(),
            ManagedChildOutput::File(file) => Stdio::from(file),
        });

        let child = command
            .spawn()
            .context("failed to spawn managed child process")?;
        Ok(Self {
            child,
            settings,
            started_at: Instant::now(),
            capture_stdout,
            capture_stderr,
        })
    }

    /// Return the spawned child PID for supervision and lifecycle logging.
    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Take ownership of the spawned child stdin handle so callers can close it after writing.
    pub(crate) fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.child.stdin.take()
    }

    /// Wait for a child whose stdout and stderr are redirected to files.
    ///
    /// Returns an error when output capture was requested or supervision fails.
    pub(crate) fn wait(
        self,
        mut on_timeout: impl FnMut(ManagedChildTimeout) -> Result<()>,
    ) -> Result<ManagedChildResult> {
        let ManagedChild {
            child,
            settings,
            started_at,
            capture_stdout,
            capture_stderr,
        } = self;

        if capture_stdout || capture_stderr {
            return Err(anyhow!(
                "managed child wait() cannot be used when stdout or stderr capture is enabled"
            ));
        }

        let mut deferred_error = None;
        let (status, timeout) = wait_for_child_exit(
            child,
            started_at,
            settings,
            &mut on_timeout,
            &mut deferred_error,
        )?;
        if let Some(error) = deferred_error {
            return Err(error);
        }
        Ok(ManagedChildResult {
            status,
            elapsed: started_at.elapsed(),
            stdout: None,
            stderr: None,
            timeout,
        })
    }

    /// Wait for a child with captured stdout and stderr, processing line callbacks as output arrives.
    ///
    /// Returns an error when the child cannot be supervised, captured streams cannot be read, or
    /// a callback fails.
    pub(crate) fn wait_with_captured_output(
        mut self,
        mut on_stdout_line: impl FnMut(&str) -> Result<()>,
        mut on_stderr_line: impl FnMut(&str) -> Result<()>,
        mut on_timeout: impl FnMut(ManagedChildTimeout) -> Result<()>,
    ) -> Result<ManagedChildResult> {
        if !self.capture_stdout || !self.capture_stderr {
            return Err(anyhow!(
                "managed child captured output requires both stdout and stderr capture"
            ));
        }

        let stdout = self
            .child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture managed child stdout"))?;
        let stderr = self
            .child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture managed child stderr"))?;
        let (sender, receiver) = mpsc::channel();
        let stdout_handle = spawn_stream_reader(stdout, StreamKind::Stdout, sender.clone());
        let stderr_handle = spawn_stream_reader(stderr, StreamKind::Stderr, sender);

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut streams_closed = StreamClosure::default();
        let mut timeout_state = None;
        let mut deferred_error: Option<anyhow::Error> = None;

        while !streams_closed.all_closed() {
            let wait_slice = timeout_wait_slice(self.started_at, self.settings, timeout_state);
            match receiver.recv_timeout(wait_slice) {
                Ok(StreamEvent::Line { kind, line }) => {
                    let buffer = match kind {
                        StreamKind::Stdout => &mut stdout,
                        StreamKind::Stderr => &mut stderr,
                    };
                    buffer.push_str(&line);
                    buffer.push('\n');

                    let callback = match kind {
                        StreamKind::Stdout => on_stdout_line(&line),
                        StreamKind::Stderr => on_stderr_line(&line),
                    };
                    if let Err(error) = callback {
                        store_first_error(&mut deferred_error, error);
                        timeout_state = Some(force_shutdown_after_callback_error(
                            &mut self.child,
                            self.started_at,
                            self.settings,
                        )?);
                    }
                }
                Ok(StreamEvent::Closed(kind)) => streams_closed.mark_closed(kind),
                Ok(StreamEvent::Error { kind, error }) => {
                    return terminate_after_stream_failure(
                        self.child,
                        self.started_at,
                        self.settings,
                        kind,
                        error,
                    );
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    streams_closed.stdout = true;
                    streams_closed.stderr = true;
                }
            }

            if timeout_state.is_none() {
                if let Some(timeout) =
                    maybe_timeout_expired(self.child.id(), self.started_at, self.settings)
                {
                    timeout_state = Some(begin_timeout_shutdown_with_callback(
                        &mut self.child,
                        timeout,
                        self.started_at,
                        &mut on_timeout,
                        &mut deferred_error,
                    )?);
                }
            }

            if let Some(status) = try_wait_for_child(&mut self.child)? {
                wait_for_streams_to_close(
                    &receiver,
                    &mut streams_closed,
                    &mut stdout,
                    &mut stderr,
                    &mut on_stdout_line,
                    &mut on_stderr_line,
                    &mut deferred_error,
                )?;
                join_stream_reader(stdout_handle, "stdout")?;
                join_stream_reader(stderr_handle, "stderr")?;
                if let Some(error) = deferred_error {
                    return Err(error);
                }
                return Ok(ManagedChildResult {
                    status,
                    elapsed: self.started_at.elapsed(),
                    stdout: Some(stdout),
                    stderr: Some(stderr),
                    timeout: timeout_state,
                });
            }

            if let Some(timeout) = timeout_state
                && timeout.termination == ManagedChildTermination::GracefulTerminated
                && timeout_elapsed_since_signal(self.started_at, timeout)
                    >= self.settings.graceful_shutdown
            {
                timeout_state = Some(escalate_timeout_shutdown(&mut self.child, timeout)?);
            }
        }

        let status = self
            .child
            .wait()
            .context("failed to reap managed child after streams closed")?;
        join_stream_reader(stdout_handle, "stdout")?;
        join_stream_reader(stderr_handle, "stderr")?;
        if let Some(error) = deferred_error {
            return Err(error);
        }
        Ok(ManagedChildResult {
            status,
            elapsed: self.started_at.elapsed(),
            stdout: Some(stdout),
            stderr: Some(stderr),
            timeout: timeout_state,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug)]
enum StreamEvent {
    Line {
        kind: StreamKind,
        line: String,
    },
    Closed(StreamKind),
    Error {
        kind: StreamKind,
        error: anyhow::Error,
    },
}

#[derive(Default)]
struct StreamClosure {
    stdout: bool,
    stderr: bool,
}

impl StreamClosure {
    fn mark_closed(&mut self, kind: StreamKind) {
        match kind {
            StreamKind::Stdout => self.stdout = true,
            StreamKind::Stderr => self.stderr = true,
        }
    }

    fn all_closed(&self) -> bool {
        self.stdout && self.stderr
    }
}

fn spawn_stream_reader<R>(
    reader: R,
    kind: StreamKind,
    sender: Sender<StreamEvent>,
) -> thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        loop {
            let mut bytes = Vec::new();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => {
                    let _ = sender.send(StreamEvent::Closed(kind));
                    break;
                }
                Ok(_) => {
                    if let Some(b'\n') = bytes.last().copied() {
                        bytes.pop();
                        if let Some(b'\r') = bytes.last().copied() {
                            bytes.pop();
                        }
                    }
                    let line = String::from_utf8_lossy(&bytes).to_string();
                    if sender.send(StreamEvent::Line { kind, line }).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(StreamEvent::Error {
                        kind,
                        error: anyhow!(error),
                    });
                    break;
                }
            }
        }
    })
}

fn join_stream_reader(handle: thread::JoinHandle<()>, stream_name: &str) -> Result<()> {
    handle
        .join()
        .map_err(|_| anyhow!("managed child {stream_name} reader thread panicked"))
}

fn wait_for_streams_to_close(
    receiver: &mpsc::Receiver<StreamEvent>,
    streams_closed: &mut StreamClosure,
    stdout: &mut String,
    stderr: &mut String,
    on_stdout_line: &mut impl FnMut(&str) -> Result<()>,
    on_stderr_line: &mut impl FnMut(&str) -> Result<()>,
    deferred_error: &mut Option<anyhow::Error>,
) -> Result<()> {
    while !streams_closed.all_closed() {
        match receiver.recv_timeout(STREAM_POLL_INTERVAL) {
            Ok(StreamEvent::Line { kind, line }) => {
                append_captured_line(kind, &line, stdout, stderr);
                if let Err(error) =
                    invoke_stream_callback(kind, &line, on_stdout_line, on_stderr_line)
                {
                    store_first_error(deferred_error, error);
                }
            }
            Ok(StreamEvent::Closed(kind)) => streams_closed.mark_closed(kind),
            Ok(StreamEvent::Error { kind, error }) => {
                return Err(error).with_context(|| {
                    format!("managed child {kind:?} reader failed while draining after child exit")
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                streams_closed.stdout = true;
                streams_closed.stderr = true;
            }
        }
    }
    Ok(())
}

fn append_captured_line(kind: StreamKind, line: &str, stdout: &mut String, stderr: &mut String) {
    match kind {
        StreamKind::Stdout => {
            stdout.push_str(line);
            stdout.push('\n');
        }
        StreamKind::Stderr => {
            stderr.push_str(line);
            stderr.push('\n');
        }
    }
}

fn invoke_stream_callback(
    kind: StreamKind,
    line: &str,
    on_stdout_line: &mut impl FnMut(&str) -> Result<()>,
    on_stderr_line: &mut impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    match kind {
        StreamKind::Stdout => on_stdout_line(line),
        StreamKind::Stderr => on_stderr_line(line),
    }
}

fn timeout_wait_slice(
    started_at: Instant,
    settings: ManagedChildSettings,
    timeout_state: Option<ManagedChildTimeout>,
) -> Duration {
    if timeout_state.is_some() {
        STREAM_POLL_INTERVAL
    } else {
        settings
            .timeout
            .checked_sub(started_at.elapsed())
            .map(|remaining| remaining.min(STREAM_POLL_INTERVAL))
            .unwrap_or(Duration::ZERO)
    }
}

fn try_wait_for_child(child: &mut Child) -> Result<Option<ExitStatus>> {
    child
        .try_wait()
        .context("failed to poll managed child process state")
}

fn maybe_timeout_expired(
    pid: u32,
    started_at: Instant,
    settings: ManagedChildSettings,
) -> Option<ManagedChildTimeout> {
    let elapsed = started_at.elapsed();
    (elapsed >= settings.timeout).then_some(ManagedChildTimeout {
        pid,
        timeout: settings.timeout,
        elapsed,
        graceful_shutdown: settings.graceful_shutdown,
        termination: ManagedChildTermination::GracefulTerminated,
    })
}

fn store_first_error(slot: &mut Option<anyhow::Error>, error: anyhow::Error) {
    if slot.is_none() {
        *slot = Some(error);
    }
}

fn begin_timeout_shutdown_with_callback(
    child: &mut Child,
    timeout: ManagedChildTimeout,
    started_at: Instant,
    on_timeout: &mut impl FnMut(ManagedChildTimeout) -> Result<()>,
    deferred_error: &mut Option<anyhow::Error>,
) -> Result<ManagedChildTimeout> {
    let timeout = begin_timeout_shutdown(child, timeout, started_at)?;
    if let Err(error) = on_timeout(timeout) {
        store_first_error(deferred_error, error);
    }
    Ok(timeout)
}

fn wait_for_child_exit(
    mut child: Child,
    started_at: Instant,
    settings: ManagedChildSettings,
    on_timeout: &mut impl FnMut(ManagedChildTimeout) -> Result<()>,
    deferred_error: &mut Option<anyhow::Error>,
) -> Result<(ExitStatus, Option<ManagedChildTimeout>)> {
    let mut timeout_state = None;
    loop {
        if let Some(status) = try_wait_for_child(&mut child)? {
            return Ok((status, timeout_state));
        }

        if timeout_state.is_none()
            && let Some(timeout) = maybe_timeout_expired(child.id(), started_at, settings)
        {
            timeout_state = Some(begin_timeout_shutdown_with_callback(
                &mut child,
                timeout,
                started_at,
                on_timeout,
                deferred_error,
            )?);
        }

        if let Some(timeout) = timeout_state
            && timeout.termination == ManagedChildTermination::GracefulTerminated
            && timeout_elapsed_since_signal(started_at, timeout) >= settings.graceful_shutdown
        {
            timeout_state = Some(escalate_timeout_shutdown(&mut child, timeout)?);
        }

        thread::sleep(STREAM_POLL_INTERVAL);
    }
}

fn begin_timeout_shutdown(
    child: &mut Child,
    timeout: ManagedChildTimeout,
    started_at: Instant,
) -> Result<ManagedChildTimeout> {
    terminate_process_group(child.id(), Signal::Terminate)?;
    Ok(ManagedChildTimeout {
        elapsed: started_at.elapsed(),
        ..timeout
    })
}

fn escalate_timeout_shutdown(
    child: &mut Child,
    timeout: ManagedChildTimeout,
) -> Result<ManagedChildTimeout> {
    terminate_process_group(child.id(), Signal::Kill)?;
    Ok(ManagedChildTimeout {
        termination: ManagedChildTermination::ForceKilled,
        ..timeout
    })
}

fn timeout_elapsed_since_signal(started_at: Instant, timeout: ManagedChildTimeout) -> Duration {
    started_at.elapsed().saturating_sub(timeout.elapsed)
}

fn force_shutdown_after_callback_error(
    child: &mut Child,
    started_at: Instant,
    settings: ManagedChildSettings,
) -> Result<ManagedChildTimeout> {
    let timeout = ManagedChildTimeout {
        pid: child.id(),
        timeout: settings.timeout,
        elapsed: started_at.elapsed(),
        graceful_shutdown: settings.graceful_shutdown,
        termination: ManagedChildTermination::ForceKilled,
    };
    terminate_process_group(child.id(), Signal::Kill)?;
    Ok(timeout)
}

fn terminate_after_stream_failure(
    mut child: Child,
    started_at: Instant,
    _settings: ManagedChildSettings,
    kind: StreamKind,
    error: anyhow::Error,
) -> Result<ManagedChildResult> {
    let timeout = force_shutdown_after_callback_error(
        &mut child,
        started_at,
        ManagedChildSettings {
            timeout: Duration::ZERO,
            graceful_shutdown: Duration::ZERO,
        },
    )?;
    let status = child
        .wait()
        .context("failed to reap managed child after stream reader failure")?;
    let _ = status;
    Err(error)
        .with_context(|| format!("failed to drain managed child {kind:?}"))
        .context(format!(
            "managed child {} was forcefully terminated after a stream reader failure",
            timeout.pid
        ))
}

#[derive(Debug, Clone, Copy)]
enum Signal {
    Terminate,
    Kill,
}

#[cfg(unix)]
fn configure_command_process_group(command: &mut Command) -> Result<()> {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn configure_command_process_group(_: &mut Command) -> Result<()> {
    Err(anyhow!(
        "managed child subprocess supervision is only supported on macOS and Linux"
    ))
}

#[cfg(unix)]
fn terminate_process_group(pid: u32, signal: Signal) -> Result<()> {
    let pid = pid as i32;
    if pid <= 0 {
        return Ok(());
    }

    let signal = match signal {
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    let result = unsafe { libc::killpg(pid, signal) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error).context("failed to signal managed child process group");
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn terminate_process_group(_: u32, _: Signal) -> Result<()> {
    Err(anyhow!(
        "managed child subprocess supervision is only supported on macOS and Linux"
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        ManagedChild, ManagedChildOutput, ManagedChildSettings, ManagedChildTermination, Signal,
        terminate_process_group,
    };
    use anyhow::anyhow;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    fn write_script(path: &Path, body: &str) {
        fs::write(path, body).expect("script should write");
        let mut permissions = fs::metadata(path)
            .expect("metadata should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("permissions should save");
    }

    fn wait_for_path(path: &Path, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if path.exists() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_process_exit(pid: u32, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if !process_is_running(pid) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for process {pid} to exit"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_file_contains(path: &Path, needle: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(contents) = fs::read_to_string(path)
                && contents.contains(needle)
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {} to contain {:?}",
                path.display(),
                needle
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn captured_output_returns_lines_and_exit_status() {
        let temp = tempdir().expect("tempdir should build");
        let script = temp.path().join("normal.sh");
        write_script(
            &script,
            "#!/bin/sh\nprintf 'hello\\n'\nprintf 'warn\\n' >&2\n",
        );

        let mut command = Command::new(&script);
        let child = ManagedChild::spawn(
            &mut command,
            ManagedChildOutput::Capture,
            ManagedChildOutput::Capture,
            ManagedChildSettings {
                timeout: Duration::from_secs(5),
                graceful_shutdown: Duration::from_secs(1),
            },
        )
        .expect("child should spawn");

        let mut stdout_lines = Vec::new();
        let mut stderr_lines = Vec::new();
        let result = child
            .wait_with_captured_output(
                |line| {
                    stdout_lines.push(line.to_string());
                    Ok(())
                },
                |line| {
                    stderr_lines.push(line.to_string());
                    Ok(())
                },
                |_| Ok(()),
            )
            .expect("child should exit cleanly");

        assert!(result.status.success());
        assert_eq!(stdout_lines, vec!["hello"]);
        assert_eq!(stderr_lines, vec!["warn"]);
        assert_eq!(result.stdout.as_deref(), Some("hello\n"));
        assert_eq!(result.stderr.as_deref(), Some("warn\n"));
        assert!(result.timeout.is_none());
    }

    #[test]
    fn timeout_kills_process_group_and_reaps_children() {
        let temp = tempdir().expect("tempdir should build");
        let script = temp.path().join("timeout.sh");
        let child_pid_file = temp.path().join("child.pid");
        write_script(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
                child_pid_file.display()
            )
            .as_str(),
        );

        let mut command = Command::new(&script);
        let child = ManagedChild::spawn(
            &mut command,
            ManagedChildOutput::Capture,
            ManagedChildOutput::Capture,
            ManagedChildSettings {
                timeout: Duration::from_millis(750),
                graceful_shutdown: Duration::from_millis(200),
            },
        )
        .expect("child should spawn");
        let parent_pid = child.pid();
        wait_for_path(&child_pid_file, Duration::from_secs(2));
        let result = child
            .wait_with_captured_output(|_| Ok(()), |_| Ok(()), |_| Ok(()))
            .expect("timeout should still return a result");

        let timeout = result.timeout.expect("timeout details should be present");
        assert_eq!(timeout.pid, parent_pid);
        assert_eq!(
            timeout.termination,
            ManagedChildTermination::GracefulTerminated
        );
        let child_pid = fs::read_to_string(child_pid_file)
            .expect("child pid should exist")
            .trim()
            .parse::<u32>()
            .expect("pid should parse");
        wait_for_process_exit(parent_pid, Duration::from_secs(2));
        wait_for_process_exit(child_pid, Duration::from_secs(2));
    }

    #[test]
    fn timeout_escalates_to_sigkill_when_sigterm_is_ignored() {
        let temp = tempdir().expect("tempdir should build");
        let script = temp.path().join("ignore-term.sh");
        let child_pid_file = temp.path().join("child.pid");
        write_script(
            &script,
            format!(
                "#!/bin/sh\ntrap '' TERM\n( trap '' TERM\nwhile :; do\n  sleep 1\ndone\n) &\nprintf '%s' \"$!\" > '{}'\nwhile :; do\n  sleep 1\ndone\n",
                child_pid_file.display()
            )
            .as_str(),
        );

        let mut command = Command::new(&script);
        let child = ManagedChild::spawn(
            &mut command,
            ManagedChildOutput::Capture,
            ManagedChildOutput::Capture,
            ManagedChildSettings {
                timeout: Duration::from_millis(500),
                graceful_shutdown: Duration::from_millis(200),
            },
        )
        .expect("child should spawn");
        let parent_pid = child.pid();
        wait_for_path(&child_pid_file, Duration::from_secs(2));
        let result = child
            .wait_with_captured_output(|_| Ok(()), |_| Ok(()), |_| Ok(()))
            .expect("timeout should still return a result");

        let timeout = result.timeout.expect("timeout details should be present");
        assert_eq!(timeout.termination, ManagedChildTermination::ForceKilled);
        let child_pid = fs::read_to_string(child_pid_file)
            .expect("child pid should exist")
            .trim()
            .parse::<u32>()
            .expect("pid should parse");
        wait_for_process_exit(parent_pid, Duration::from_secs(2));
        wait_for_process_exit(child_pid, Duration::from_secs(2));
    }

    #[test]
    fn timeout_callback_failure_still_reaps_captured_process_group() {
        let temp = tempdir().expect("tempdir should build");
        let script = temp.path().join("callback-error.sh");
        let child_pid_file = temp.path().join("child.pid");
        write_script(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
                child_pid_file.display()
            )
            .as_str(),
        );

        let mut command = Command::new(&script);
        let child = ManagedChild::spawn(
            &mut command,
            ManagedChildOutput::Capture,
            ManagedChildOutput::Capture,
            ManagedChildSettings {
                timeout: Duration::from_millis(750),
                graceful_shutdown: Duration::from_millis(200),
            },
        )
        .expect("child should spawn");
        let parent_pid = child.pid();
        wait_for_path(&child_pid_file, Duration::from_secs(2));
        let error = child
            .wait_with_captured_output(
                |_| Ok(()),
                |_| Ok(()),
                |_| Err(anyhow!("timeout callback failed")),
            )
            .expect_err("timeout callback failure should surface");

        let child_pid = fs::read_to_string(child_pid_file)
            .expect("child pid should exist")
            .trim()
            .parse::<u32>()
            .expect("pid should parse");
        assert!(error.to_string().contains("timeout callback failed"));
        wait_for_process_exit(parent_pid, Duration::from_secs(2));
        wait_for_process_exit(child_pid, Duration::from_secs(2));
    }

    #[test]
    fn timeout_callback_failure_still_reaps_redirected_process_group() {
        let temp = tempdir().expect("tempdir should build");
        let script = temp.path().join("callback-error-redirected.sh");
        let child_pid_file = temp.path().join("child.pid");
        let stdout_path = temp.path().join("stdout.log");
        let stderr_path = temp.path().join("stderr.log");
        write_script(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
                child_pid_file.display()
            )
            .as_str(),
        );

        let mut command = Command::new(&script);
        let child = ManagedChild::spawn(
            &mut command,
            ManagedChildOutput::File(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&stdout_path)
                    .expect("stdout log should open"),
            ),
            ManagedChildOutput::File(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&stderr_path)
                    .expect("stderr log should open"),
            ),
            ManagedChildSettings {
                timeout: Duration::from_millis(750),
                graceful_shutdown: Duration::from_millis(200),
            },
        )
        .expect("child should spawn");
        let parent_pid = child.pid();
        wait_for_path(&child_pid_file, Duration::from_secs(2));
        let error = child
            .wait(|_| Err(anyhow!("timeout callback failed")))
            .expect_err("timeout callback failure should surface");

        let child_pid = fs::read_to_string(child_pid_file)
            .expect("child pid should exist")
            .trim()
            .parse::<u32>()
            .expect("pid should parse");
        assert!(error.to_string().contains("timeout callback failed"));
        wait_for_process_exit(parent_pid, Duration::from_secs(2));
        wait_for_process_exit(child_pid, Duration::from_secs(2));
    }

    #[test]
    fn redirected_output_uses_same_timeout_contract() {
        let temp = tempdir().expect("tempdir should build");
        let script = temp.path().join("redirected.sh");
        let stdout_path = temp.path().join("stdout.log");
        let stderr_path = temp.path().join("stderr.log");
        write_script(
            &script,
            "#!/bin/sh\nprintf 'stdout line\\n'\nprintf 'stderr line\\n' >&2\nsleep 30\n",
        );

        let mut command = Command::new(&script);
        let child = ManagedChild::spawn(
            &mut command,
            ManagedChildOutput::File(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&stdout_path)
                    .expect("stdout log should open"),
            ),
            ManagedChildOutput::File(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&stderr_path)
                    .expect("stderr log should open"),
            ),
            ManagedChildSettings {
                timeout: Duration::from_secs(3),
                graceful_shutdown: Duration::from_millis(200),
            },
        )
        .expect("child should spawn");
        wait_for_file_contains(&stdout_path, "stdout line", Duration::from_secs(2));
        wait_for_file_contains(&stderr_path, "stderr line", Duration::from_secs(2));
        let result = child
            .wait(|_| Ok(()))
            .expect("timeout should return a result");

        assert!(result.timeout.is_some());
        assert!(
            fs::read_to_string(stdout_path)
                .expect("stdout log should read")
                .contains("stdout line")
        );
        assert!(
            fs::read_to_string(stderr_path)
                .expect("stderr log should read")
                .contains("stderr line")
        );
    }

    #[test]
    fn esrch_during_process_group_cleanup_is_non_fatal() {
        terminate_process_group(999_999, Signal::Kill).expect("esrch should not fail");
    }

    #[cfg(unix)]
    fn process_is_running(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as i32, 0) };
        if result == 0 {
            true
        } else {
            std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        }
    }
}
