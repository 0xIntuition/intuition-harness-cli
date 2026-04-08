use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestPublishMode {
    Ready,
    Draft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestLifecycleAction {
    CreatedReady,
    CreatedDraft,
    UpdatedExisting,
    PromotedToReady,
    AlreadyReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestLifecycleResult {
    pub(crate) number: u64,
    pub(crate) url: String,
    pub(crate) action: PullRequestLifecycleAction,
    pub(crate) is_draft: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PullRequestPublishRequest<'a> {
    pub(crate) head_branch: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) title: &'a str,
    pub(crate) body_path: &'a Path,
    pub(crate) mode: PullRequestPublishMode,
}

#[derive(Debug, Clone)]
pub(crate) struct GhCli;

#[derive(Debug, Clone, Deserialize)]
struct BranchPullRequest {
    number: u64,
    url: String,
    #[serde(rename = "isDraft", default)]
    is_draft: bool,
    #[serde(rename = "headRefName", default)]
    head_ref_name: Option<String>,
    #[serde(rename = "headRefOid", default)]
    head_ref_oid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestCheckClassification {
    Pending,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestChecksOutcome {
    NoChecksConfigured,
    Pending,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestCheck {
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) bucket: String,
    pub(crate) description: Option<String>,
    pub(crate) link: Option<String>,
    pub(crate) classification: PullRequestCheckClassification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestChecksSnapshot {
    pub(crate) outcome: PullRequestChecksOutcome,
    pub(crate) settled_count: usize,
    pub(crate) total_count: usize,
    pub(crate) checks: Vec<PullRequestCheck>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawPullRequestCheck {
    name: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    bucket: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct WorkflowRun {
    #[serde(rename = "headSha", default)]
    pub(crate) head_sha: String,
    #[serde(default)]
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) conclusion: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(rename = "workflowName", default)]
    pub(crate) workflow_name: Option<String>,
}

impl PullRequestCheck {
    fn is_pending(&self) -> bool {
        self.classification == PullRequestCheckClassification::Pending
    }

    fn is_failed(&self) -> bool {
        self.classification == PullRequestCheckClassification::Failed
    }
}

impl From<RawPullRequestCheck> for PullRequestCheck {
    fn from(value: RawPullRequestCheck) -> Self {
        Self {
            classification: classify_pull_request_check(&value.state, &value.bucket),
            name: value.name,
            state: value.state,
            bucket: value.bucket,
            description: value.description,
            link: value.link,
        }
    }
}

impl PullRequestChecksSnapshot {
    fn from_checks(checks: Vec<PullRequestCheck>) -> Self {
        let total_count = checks.len();
        let settled_count = checks.iter().filter(|check| !check.is_pending()).count();
        let outcome = if checks.is_empty() {
            PullRequestChecksOutcome::NoChecksConfigured
        } else if checks.iter().any(PullRequestCheck::is_failed) {
            PullRequestChecksOutcome::Failed
        } else if settled_count < total_count {
            PullRequestChecksOutcome::Pending
        } else {
            PullRequestChecksOutcome::Passed
        };

        Self {
            outcome,
            settled_count,
            total_count,
            checks,
        }
    }

    /// Returns the failing checks in the current snapshot.
    pub(crate) fn failed_checks(&self) -> Vec<PullRequestCheck> {
        self.checks
            .iter()
            .filter(|check| check.is_failed())
            .cloned()
            .collect()
    }
}

fn classify_pull_request_check(state: &str, bucket: &str) -> PullRequestCheckClassification {
    classify_status_token(bucket)
        .or_else(|| classify_status_token(state))
        .unwrap_or(PullRequestCheckClassification::Pending)
}

fn classify_status_token(token: &str) -> Option<PullRequestCheckClassification> {
    let normalized = normalize_status_token(token);
    if normalized.is_empty() {
        return None;
    }

    if matches!(
        normalized.as_str(),
        "fail"
            | "failure"
            | "failed"
            | "cancel"
            | "cancelled"
            | "canceled"
            | "timed_out"
            | "timeout"
            | "error"
            | "startup_failure"
            | "action_required"
    ) {
        return Some(PullRequestCheckClassification::Failed);
    }

    if matches!(
        normalized.as_str(),
        "pending"
            | "queued"
            | "queue"
            | "in_progress"
            | "inprogress"
            | "requested"
            | "waiting"
            | "waiting_on"
    ) {
        return Some(PullRequestCheckClassification::Pending);
    }

    if matches!(
        normalized.as_str(),
        "pass"
            | "passed"
            | "success"
            | "successful"
            | "neutral"
            | "skip"
            | "skipped"
            | "skipping"
            | "complete"
            | "completed"
    ) {
        return Some(PullRequestCheckClassification::Passed);
    }

    None
}

fn normalize_status_token(token: &str) -> String {
    token
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

impl GhCli {
    fn view_pull_request_by_number(
        &self,
        workspace_path: &Path,
        number: u64,
    ) -> Result<BranchPullRequest> {
        self.run_json::<BranchPullRequest>(
            workspace_path,
            &[
                "pr",
                "view",
                &number.to_string(),
                "--json",
                "number,url,isDraft",
            ],
        )
    }

    /// Run `gh` and deserialize its JSON output.
    ///
    /// Returns an error when the command cannot be launched, exits unsuccessfully,
    /// or emits invalid JSON.
    pub(crate) fn run_json<T: DeserializeOwned>(&self, root: &Path, args: &[&str]) -> Result<T> {
        let output = Command::new("gh")
            .args(args)
            .current_dir(root)
            .output()
            .with_context(|| format!("failed to run `gh {}`", args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "gh {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("failed to decode JSON from `gh {}`", args.join(" ")))
    }

    /// Run `gh` without expecting JSON output.
    ///
    /// Returns an error when the command cannot be launched or exits unsuccessfully.
    pub(crate) fn run_plain(&self, root: &Path, args: &[&str]) -> Result<()> {
        let output = Command::new("gh")
            .args(args)
            .current_dir(root)
            .output()
            .with_context(|| format!("failed to run `gh {}`", args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "gh {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Create a branch pull request or update the existing open PR for the same head/base pair.
    ///
    /// Returns an error when `gh` cannot inspect, create, or edit the pull request.
    pub(crate) fn publish_branch_pull_request(
        &self,
        workspace_path: &Path,
        request: PullRequestPublishRequest<'_>,
    ) -> Result<PullRequestLifecycleResult> {
        if let Some(existing) = self.find_open_branch_pull_request_raw(
            workspace_path,
            request.head_branch,
            request.base_branch,
        )? {
            self.run_plain(
                workspace_path,
                &[
                    "pr",
                    "edit",
                    &existing.number.to_string(),
                    "--title",
                    request.title,
                    "--body-file",
                    body_path_arg(request.body_path)?,
                ],
            )?;
            return Ok(PullRequestLifecycleResult {
                number: existing.number,
                url: existing.url,
                action: PullRequestLifecycleAction::UpdatedExisting,
                is_draft: existing.is_draft,
            });
        }

        let mut create_args = vec![
            "pr",
            "create",
            "--base",
            request.base_branch,
            "--head",
            request.head_branch,
            "--title",
            request.title,
            "--body-file",
            body_path_arg(request.body_path)?,
        ];
        if request.mode == PullRequestPublishMode::Draft {
            create_args.push("--draft");
        }
        let created = self
            .run_json::<BranchPullRequest>(
                workspace_path,
                &[&create_args[..], &["--json", "number,url,isDraft"]].concat(),
            )
            .or_else(|_| {
                self.run_plain(workspace_path, &create_args)?;
                self.find_open_branch_pull_request_raw(
                    workspace_path,
                    request.head_branch,
                    request.base_branch,
                )?
                .ok_or_else(|| {
                    anyhow!(
                        "gh created a pull request for `{}` but no open PR was returned",
                        request.head_branch
                    )
                })
            })?;

        Ok(PullRequestLifecycleResult {
            number: created.number,
            url: created.url,
            action: match request.mode {
                PullRequestPublishMode::Ready => PullRequestLifecycleAction::CreatedReady,
                PullRequestPublishMode::Draft => PullRequestLifecycleAction::CreatedDraft,
            },
            is_draft: created.is_draft,
        })
    }

    /// Refresh the title/body for the existing open PR matching the provided head/base pair.
    ///
    /// Returns `Ok(None)` when no matching open PR exists, and an error when `gh` fails to inspect
    /// or edit the existing pull request.
    pub(crate) fn refresh_existing_branch_pull_request(
        &self,
        workspace_path: &Path,
        request: PullRequestPublishRequest<'_>,
    ) -> Result<Option<PullRequestLifecycleResult>> {
        let Some(existing) = self.find_open_branch_pull_request_raw(
            workspace_path,
            request.head_branch,
            request.base_branch,
        )?
        else {
            return Ok(None);
        };

        self.run_plain(
            workspace_path,
            &[
                "pr",
                "edit",
                &existing.number.to_string(),
                "--title",
                request.title,
                "--body-file",
                body_path_arg(request.body_path)?,
            ],
        )?;

        Ok(Some(PullRequestLifecycleResult {
            number: existing.number,
            url: existing.url,
            action: PullRequestLifecycleAction::UpdatedExisting,
            is_draft: existing.is_draft,
        }))
    }

    /// Refresh the title/body for the specified open pull request number.
    ///
    /// Returns an error when `gh` fails to edit the existing pull request.
    pub(crate) fn refresh_pull_request_by_number(
        &self,
        workspace_path: &Path,
        number: u64,
        title: &str,
        body_path: &Path,
    ) -> Result<PullRequestLifecycleResult> {
        self.run_plain(
            workspace_path,
            &[
                "pr",
                "edit",
                &number.to_string(),
                "--title",
                title,
                "--body-file",
                body_path_arg(body_path)?,
            ],
        )?;

        let refreshed = self.view_pull_request_by_number(workspace_path, number)?;

        Ok(PullRequestLifecycleResult {
            number: refreshed.number,
            url: refreshed.url,
            action: PullRequestLifecycleAction::UpdatedExisting,
            is_draft: refreshed.is_draft,
        })
    }

    /// Promote the provided open pull request number to ready for review.
    ///
    /// Returns an error when `gh` cannot inspect or promote the pull request.
    pub(crate) fn promote_pull_request_to_ready(
        &self,
        workspace_path: &Path,
        number: u64,
    ) -> Result<PullRequestLifecycleResult> {
        let existing = self.view_pull_request_by_number(workspace_path, number)?;
        if !existing.is_draft {
            return Ok(PullRequestLifecycleResult {
                number: existing.number,
                url: existing.url,
                action: PullRequestLifecycleAction::AlreadyReady,
                is_draft: false,
            });
        }

        self.run_plain(workspace_path, &["pr", "ready", &number.to_string()])?;
        let ready = self.view_pull_request_by_number(workspace_path, number)?;
        if ready.is_draft {
            bail!("pull request #{number} is still draft after `gh pr ready`");
        }
        Ok(PullRequestLifecycleResult {
            number: ready.number,
            url: ready.url,
            action: PullRequestLifecycleAction::PromotedToReady,
            is_draft: false,
        })
    }

    /// Ensure the requested label exists in the repository.
    ///
    /// Returns an error when `gh` cannot create the label for reasons other than it already existing.
    pub(crate) fn ensure_label_exists(
        &self,
        workspace_path: &Path,
        label: &str,
        color: &str,
        description: &str,
    ) -> Result<()> {
        match self.run_plain(
            workspace_path,
            &[
                "label",
                "create",
                label,
                "--color",
                color,
                "--description",
                description,
            ],
        ) {
            Ok(()) => Ok(()),
            Err(error) if error.to_string().contains("already exists") => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Add a label to the provided pull request number.
    ///
    /// Returns an error when `gh` cannot edit the pull request.
    pub(crate) fn add_label_to_pull_request(
        &self,
        workspace_path: &Path,
        number: u64,
        label: &str,
    ) -> Result<()> {
        self.run_plain(
            workspace_path,
            &["pr", "edit", &number.to_string(), "--add-label", label],
        )
    }

    /// Returns a structured snapshot of CI or status checks for the provided pull request number.
    ///
    /// Returns an error when `gh` cannot inspect the pull request checks.
    pub(crate) fn pull_request_checks_snapshot(
        &self,
        workspace_path: &Path,
        number: u64,
    ) -> Result<PullRequestChecksSnapshot> {
        let args = [
            "pr",
            "checks",
            &number.to_string(),
            "--json",
            "name,state,bucket,description,link",
        ];
        let output = Command::new("gh")
            .args(args)
            .current_dir(workspace_path)
            .output()
            .with_context(|| format!("failed to run `gh {}`", args.join(" ")))?;
        let exit_code = output
            .status
            .code()
            .ok_or_else(|| anyhow!("gh {} terminated without an exit code", args.join(" ")))?;

        decode_pull_request_checks_snapshot(
            &args.join(" "),
            exit_code,
            &output.stdout,
            &output.stderr,
        )
    }

    /// Resolve the active open branch PR for the provided head/base pair.
    ///
    /// Returns `Ok(None)` when no open PR matches the branch, and an error when `gh`
    /// cannot inspect the repository pull requests.
    pub(crate) fn find_open_branch_pull_request(
        &self,
        workspace_path: &Path,
        head_branch: &str,
        base_branch: &str,
    ) -> Result<Option<ResolvedBranchPullRequest>> {
        Ok(self
            .find_open_branch_pull_request_raw(workspace_path, head_branch, base_branch)?
            .map(ResolvedBranchPullRequest::from))
    }

    /// List workflow runs for an exact commit SHA and workflow name.
    ///
    /// Returns an error when `gh` cannot inspect workflow runs.
    pub(crate) fn list_workflow_runs_for_commit(
        &self,
        workspace_path: &Path,
        workflow_name: &str,
        head_sha: &str,
    ) -> Result<Vec<WorkflowRun>> {
        self.run_json::<Vec<WorkflowRun>>(
            workspace_path,
            &[
                "run",
                "list",
                "--commit",
                head_sha,
                "--workflow",
                workflow_name,
                "--json",
                "headSha,status,conclusion,url,workflowName",
            ],
        )
    }

    fn find_open_branch_pull_request_raw(
        &self,
        workspace_path: &Path,
        head_branch: &str,
        base_branch: &str,
    ) -> Result<Option<BranchPullRequest>> {
        let existing = self.run_json::<Vec<BranchPullRequest>>(
            workspace_path,
            &[
                "pr",
                "list",
                "--state",
                "open",
                "--head",
                head_branch,
                "--base",
                base_branch,
                "--json",
                "number,url,isDraft,headRefName,headRefOid",
            ],
        )?;
        Ok(existing.into_iter().next())
    }
}

fn decode_pull_request_checks_snapshot(
    command_display: &str,
    exit_code: i32,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<PullRequestChecksSnapshot> {
    let stderr_text = String::from_utf8_lossy(stderr).trim().to_string();
    if stdout.is_empty() {
        if stderr_text.is_empty() {
            bail!("gh {command_display} returned no JSON output");
        }
        bail!("gh {command_display} failed: {stderr_text}");
    }

    let checks = serde_json::from_slice::<Vec<RawPullRequestCheck>>(stdout)
        .with_context(|| format!("failed to decode JSON from `gh {command_display}`"))?
        .into_iter()
        .map(PullRequestCheck::from)
        .collect();
    let snapshot = PullRequestChecksSnapshot::from_checks(checks);

    match exit_code {
        0 => Ok(snapshot),
        8 if snapshot.outcome == PullRequestChecksOutcome::Pending => Ok(snapshot),
        code if code != 0 && snapshot.outcome == PullRequestChecksOutcome::Failed => Ok(snapshot),
        code => {
            let stderr_suffix = if stderr_text.is_empty() {
                String::new()
            } else {
                format!(": {stderr_text}")
            };
            bail!(
                "gh {command_display} returned exit code {code} for {:?}{stderr_suffix}",
                snapshot.outcome
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedBranchPullRequest {
    pub(crate) number: u64,
    pub(crate) url: String,
    pub(crate) is_draft: bool,
    pub(crate) head_ref_name: Option<String>,
    pub(crate) head_ref_oid: Option<String>,
}

impl From<BranchPullRequest> for ResolvedBranchPullRequest {
    fn from(value: BranchPullRequest) -> Self {
        Self {
            number: value.number,
            url: value.url,
            is_draft: value.is_draft,
            head_ref_name: value.head_ref_name,
            head_ref_oid: value.head_ref_oid,
        }
    }
}

fn body_path_arg(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("invalid PR body path `{}`", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        PullRequestCheck, PullRequestCheckClassification, PullRequestChecksOutcome,
        PullRequestChecksSnapshot, classify_pull_request_check,
        decode_pull_request_checks_snapshot,
    };

    fn check(name: &str, state: &str, bucket: &str) -> PullRequestCheck {
        PullRequestCheck {
            name: name.to_string(),
            state: state.to_string(),
            bucket: bucket.to_string(),
            description: None,
            link: None,
            classification: classify_pull_request_check(state, bucket),
        }
    }

    #[test]
    fn pull_request_checks_snapshot_reports_no_checks_when_empty() {
        let snapshot = PullRequestChecksSnapshot::from_checks(Vec::new());

        assert_eq!(
            snapshot.outcome,
            PullRequestChecksOutcome::NoChecksConfigured
        );
        assert_eq!(snapshot.total_count, 0);
        assert_eq!(snapshot.settled_count, 0);
    }

    #[test]
    fn pull_request_checks_snapshot_reports_pending_when_any_check_is_unsettled() {
        let snapshot = PullRequestChecksSnapshot::from_checks(vec![
            check("ci / quality", "IN_PROGRESS", "pending"),
            check("ci / docs", "SUCCESS", "pass"),
        ]);

        assert_eq!(snapshot.outcome, PullRequestChecksOutcome::Pending);
        assert_eq!(snapshot.total_count, 2);
        assert_eq!(snapshot.settled_count, 1);
    }

    #[test]
    fn pull_request_checks_snapshot_reports_failed_for_cancelled_or_timed_out_checks() {
        let cancelled = check("ci / cancelled", "CANCELLED", "");
        let timed_out = check("ci / timeout", "TIMED_OUT", "");

        assert_eq!(
            cancelled.classification,
            PullRequestCheckClassification::Failed
        );
        assert_eq!(
            timed_out.classification,
            PullRequestCheckClassification::Failed
        );

        let snapshot = PullRequestChecksSnapshot::from_checks(vec![cancelled, timed_out]);
        assert_eq!(snapshot.outcome, PullRequestChecksOutcome::Failed);
        assert_eq!(snapshot.failed_checks().len(), 2);
    }

    #[test]
    fn pull_request_checks_snapshot_reports_passed_when_all_checks_settle_green() {
        let snapshot = PullRequestChecksSnapshot::from_checks(vec![
            check("ci / quality", "SUCCESS", "pass"),
            check("ci / docs", "SKIPPED", "skipping"),
        ]);

        assert_eq!(snapshot.outcome, PullRequestChecksOutcome::Passed);
        assert_eq!(snapshot.total_count, 2);
        assert_eq!(snapshot.settled_count, 2);
    }

    #[test]
    fn unknown_check_tokens_fail_closed_as_pending() {
        assert_eq!(
            classify_pull_request_check("MYSTERY_STATUS", "mystery_bucket"),
            PullRequestCheckClassification::Pending
        );
    }

    #[test]
    fn pull_request_checks_snapshot_accepts_pending_exit_code_eight_when_json_is_present() {
        let snapshot = decode_pull_request_checks_snapshot(
            "pr checks 321 --json name,state,bucket,description,link",
            8,
            br#"[{"name":"ci / quality","state":"IN_PROGRESS","bucket":"pending","description":"quality gate still running","link":"https://github.com/example/repo/actions/runs/1"}]"#,
            b"",
        )
        .expect("pending exit code with JSON payload should decode");

        assert_eq!(snapshot.outcome, PullRequestChecksOutcome::Pending);
        assert_eq!(snapshot.total_count, 1);
        assert_eq!(snapshot.settled_count, 0);
    }

    #[test]
    fn pull_request_checks_snapshot_accepts_failed_non_zero_exit_when_json_is_present() {
        let snapshot = decode_pull_request_checks_snapshot(
            "pr checks 321 --json name,state,bucket,description,link",
            1,
            br#"[{"name":"ci / quality","state":"FAILURE","bucket":"fail","description":"quality gate failed","link":"https://github.com/example/repo/actions/runs/1"}]"#,
            b"",
        )
        .expect("failed non-zero exit with JSON payload should decode");

        assert_eq!(snapshot.outcome, PullRequestChecksOutcome::Failed);
        assert_eq!(snapshot.failed_checks().len(), 1);
    }

    #[test]
    fn pull_request_checks_snapshot_rejects_mismatched_pending_exit_code_and_green_payload() {
        let error = decode_pull_request_checks_snapshot(
            "pr checks 321 --json name,state,bucket,description,link",
            8,
            br#"[{"name":"ci / quality","state":"SUCCESS","bucket":"pass","description":"quality gate passed","link":"https://github.com/example/repo/actions/runs/1"}]"#,
            b"",
        )
        .expect_err("pending exit code with green payload should fail closed");

        assert!(
            error
                .to_string()
                .contains("returned exit code 8 for Passed")
        );
    }
}
