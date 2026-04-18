use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::fs::PlanningPaths;
use crate::session_runtime::{ActiveSessionFile, read_json};

use super::now_epoch_seconds;
use super::store::pid_is_running;

const LISTEN_WORKER_LEASE_VERSION: u8 = 1;
const LISTEN_WORKER_LEASE_FILE: &str = "listen-worker.lock.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ListenWorkerLease {
    pub(super) version: u8,
    pub(super) issue_identifier: String,
    pub(super) workspace_path: String,
    pub(super) source_root: String,
    #[serde(default)]
    pub(super) project_selector: Option<String>,
    pub(super) worker_pid: u32,
    #[serde(default)]
    pub(super) parent_pid: Option<u32>,
    pub(super) acquired_at_epoch_seconds: u64,
    pub(super) spawn_reason: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ListenWorkerLeaseRequest<'a> {
    pub(super) source_root: &'a Path,
    pub(super) project_selector: Option<&'a str>,
    pub(super) workspace_path: &'a Path,
    pub(super) issue_identifier: &'a str,
    pub(super) worker_pid: u32,
    pub(super) parent_pid: Option<u32>,
    pub(super) spawn_reason: &'a str,
}

#[derive(Debug)]
pub(super) struct ListenWorkerLeaseGuard {
    path: PathBuf,
    lease: ListenWorkerLease,
}

impl ListenWorkerLeaseGuard {
    /// Return the acquired workspace worker lease metadata.
    pub(super) fn lease(&self) -> &ListenWorkerLease {
        &self.lease
    }
}

impl Drop for ListenWorkerLeaseGuard {
    fn drop(&mut self) {
        match read_json::<ListenWorkerLease>(&self.path) {
            Ok(existing) if existing == self.lease => {
                if let Err(error) = fs::remove_file(&self.path) {
                    eprintln!(
                        "warning: failed to remove listen worker lease `{}` for {} pid {}: {error:#}",
                        self.path.display(),
                        self.lease.issue_identifier,
                        self.lease.worker_pid
                    );
                }
            }
            Ok(_) | Err(_) => {}
        }
    }
}

/// Acquire exclusive ownership of a listen workspace for one worker process.
///
/// Returns an error when another live worker PID already owns the workspace lease, or when the
/// lease file cannot be created after stale cleanup.
pub(super) fn acquire_listen_worker_lease(
    request: ListenWorkerLeaseRequest<'_>,
) -> Result<ListenWorkerLeaseGuard> {
    let path = listen_worker_lease_path(request.workspace_path);
    let lease = ListenWorkerLease {
        version: LISTEN_WORKER_LEASE_VERSION,
        issue_identifier: request.issue_identifier.to_string(),
        workspace_path: request.workspace_path.display().to_string(),
        source_root: request.source_root.display().to_string(),
        project_selector: request.project_selector.map(str::to_string),
        worker_pid: request.worker_pid,
        parent_pid: request.parent_pid,
        acquired_at_epoch_seconds: now_epoch_seconds(),
        spawn_reason: request.spawn_reason.to_string(),
    };

    for _ in 0..2 {
        ensure_lease_parent(&path)?;
        let file = ActiveSessionFile::<ListenWorkerLease>::new(path.clone());
        if file.try_create_new(&lease)? {
            return Ok(ListenWorkerLeaseGuard {
                path,
                lease: lease.clone(),
            });
        }

        match read_json::<ListenWorkerLease>(&path) {
            Ok(existing) if lease_owner_is_running(&existing) => {
                bail!(
                    "listen worker lease for workspace `{}` is already owned by live pid {} for issue `{}` (spawn reason `{}`)",
                    existing.workspace_path,
                    existing.worker_pid,
                    existing.issue_identifier,
                    existing.spawn_reason
                );
            }
            Ok(existing) => {
                remove_stale_lease(&path, Some(existing.worker_pid))?;
            }
            Err(_) => {
                remove_stale_lease(&path, None)?;
            }
        }
    }

    bail!(
        "failed to acquire listen worker lease `{}` for issue `{}`",
        path.display(),
        request.issue_identifier
    )
}

/// Ensure a workspace has no live listen-worker lease before launching a replacement.
///
/// Returns an error when a live worker already owns the lease. Stale or malformed leases are
/// removed so a replacement worker can acquire a fresh lease itself.
pub(super) fn ensure_listen_worker_lease_available(
    workspace_path: &Path,
    issue_identifier: &str,
) -> Result<()> {
    let path = listen_worker_lease_path(workspace_path);
    if !path.exists() {
        return Ok(());
    }
    let Some(existing) = load_listen_worker_lease(workspace_path) else {
        return remove_stale_lease(&path, None);
    };
    if lease_owner_is_running(&existing) {
        bail!(
            "workspace `{}` already has an active listen worker lease owned by pid {} for issue `{}`; refusing to launch another worker for `{}`",
            existing.workspace_path,
            existing.worker_pid,
            existing.issue_identifier,
            issue_identifier
        );
    }
    remove_stale_lease(&path, Some(existing.worker_pid))
}

/// Load the current listen-worker lease for a workspace.
///
/// Returns `Ok(None)` when the lease file is absent or malformed.
pub(super) fn load_listen_worker_lease(workspace_path: &Path) -> Option<ListenWorkerLease> {
    let path = listen_worker_lease_path(workspace_path);
    if !path.exists() {
        return None;
    }
    read_json::<ListenWorkerLease>(&path).ok()
}

fn listen_worker_lease_path(workspace_path: &Path) -> PathBuf {
    PlanningPaths::new(workspace_path)
        .metastack_dir
        .join(LISTEN_WORKER_LEASE_FILE)
}

fn ensure_lease_parent(path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create listen worker lease dir `{}`",
            parent.display()
        )
    })
}

fn lease_owner_is_running(lease: &ListenWorkerLease) -> bool {
    lease.worker_pid > 0 && pid_is_running(lease.worker_pid)
}

fn remove_stale_lease(path: &Path, pid: Option<u32>) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            let pid_label = pid
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "failed to remove stale listen worker lease `{}` for pid {}",
                path.display(),
                pid_label
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ListenWorkerLease, ListenWorkerLeaseRequest, acquire_listen_worker_lease,
        ensure_listen_worker_lease_available, load_listen_worker_lease,
    };
    use crate::session_runtime::write_json;
    use tempfile::tempdir;

    #[test]
    fn worker_lease_rejects_second_live_owner() {
        let temp = tempdir().expect("temp dir should exist");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace should exist");
        let source_root = temp.path().join("source");
        std::fs::create_dir_all(&source_root).expect("source root should exist");
        let pid = std::process::id();

        let _guard = acquire_listen_worker_lease(ListenWorkerLeaseRequest {
            source_root: &source_root,
            project_selector: Some("Inbox"),
            workspace_path: &workspace,
            issue_identifier: "ENG-11216",
            worker_pid: pid,
            parent_pid: Some(pid),
            spawn_reason: "test",
        })
        .expect("first lease should acquire");

        let error = acquire_listen_worker_lease(ListenWorkerLeaseRequest {
            source_root: &source_root,
            project_selector: Some("Inbox"),
            workspace_path: &workspace,
            issue_identifier: "ENG-11216",
            worker_pid: pid,
            parent_pid: Some(pid),
            spawn_reason: "test",
        })
        .expect_err("second live lease should be rejected");

        assert!(
            error.to_string().contains("already owned by live pid"),
            "{error:#}"
        );
    }

    #[test]
    fn stale_worker_lease_is_removed_before_launch() {
        let temp = tempdir().expect("temp dir should exist");
        let workspace = temp.path().join("workspace");
        let source_root = temp.path().join("source");
        std::fs::create_dir_all(workspace.join(crate::branding::PROJECT_DIR))
            .expect("workspace state should exist");
        std::fs::create_dir_all(&source_root).expect("source root should exist");
        let lease_path = workspace
            .join(crate::branding::PROJECT_DIR)
            .join("listen-worker.lock.json");
        write_json(
            &lease_path,
            &ListenWorkerLease {
                version: 1,
                issue_identifier: "ENG-OLD".to_string(),
                workspace_path: workspace.display().to_string(),
                source_root: source_root.display().to_string(),
                project_selector: None,
                worker_pid: 0,
                parent_pid: None,
                acquired_at_epoch_seconds: 1,
                spawn_reason: "stale-test".to_string(),
            },
        )
        .expect("stale lease should write");

        ensure_listen_worker_lease_available(&workspace, "ENG-11216")
            .expect("stale lease should be cleared");

        assert!(load_listen_worker_lease(&workspace).is_none());
    }

    #[test]
    fn worker_lease_guard_does_not_remove_replaced_owner() {
        let temp = tempdir().expect("temp dir should exist");
        let workspace = temp.path().join("workspace");
        let source_root = temp.path().join("source");
        std::fs::create_dir_all(&workspace).expect("workspace should exist");
        std::fs::create_dir_all(&source_root).expect("source root should exist");
        let guard = acquire_listen_worker_lease(ListenWorkerLeaseRequest {
            source_root: &source_root,
            project_selector: None,
            workspace_path: &workspace,
            issue_identifier: "ENG-11216",
            worker_pid: std::process::id(),
            parent_pid: None,
            spawn_reason: "test",
        })
        .expect("lease should acquire");
        let replacement = ListenWorkerLease {
            version: 1,
            issue_identifier: "ENG-OTHER".to_string(),
            workspace_path: workspace.display().to_string(),
            source_root: source_root.display().to_string(),
            project_selector: None,
            worker_pid: 0,
            parent_pid: None,
            acquired_at_epoch_seconds: 2,
            spawn_reason: "replacement".to_string(),
        };
        let lease_path = workspace
            .join(crate::branding::PROJECT_DIR)
            .join("listen-worker.lock.json");
        write_json(&lease_path, &replacement).expect("replacement should write");

        drop(guard);

        let persisted = load_listen_worker_lease(&workspace).expect("replacement should remain");
        assert_eq!(persisted.issue_identifier, "ENG-OTHER");
    }
}
