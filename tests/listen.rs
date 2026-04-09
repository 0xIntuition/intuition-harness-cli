#![allow(dead_code, unused_imports)]

include!("support/common.rs");

use metastack_cli::branding;

#[cfg(unix)]
const EXACT_SHA_QUALITY_CRITERION: &str =
    "The active branch PR has a passing `quality` workflow result for the exact current HEAD SHA.";

#[cfg(unix)]
fn write_onboarded_config(
    config_path: &Path,
    config: impl AsRef<str>,
) -> Result<(), Box<dyn Error>> {
    fs::write(
        config_path,
        format!(
            "{}\n[onboarding]\ncompleted = true\n",
            config.as_ref().trim_end()
        ),
    )?;
    Ok(())
}

#[cfg(unix)]
fn write_codex_global_config(home_dir: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(home_dir.join(".codex"))?;
    fs::write(
        home_dir.join(".codex/config.toml"),
        r#"approval_policy = "never"
sandbox_mode = "danger-full-access"

[mcp_servers.linear]
enabled = true
"#,
    )?;
    Ok(())
}

#[cfg(unix)]
fn write_codex_help_stub(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  cat <<'EOF'
-a, --ask-for-approval <APPROVAL_POLICY>
-s, --sandbox <SANDBOX_MODE>
-C, --cd <DIR>
    --add-dir <DIR>
    --dangerously-bypass-approvals-and-sandbox
EOF
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-m, --model <MODEL>
-c, --config <key=value>
    --json
EOF
  exit 0
fi
exit 0
"#,
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn write_listen_github_stub(
    path: &Path,
    initial_state: &str,
    pull_request_url: &str,
) -> Result<(), Box<dyn Error>> {
    write_listen_github_stub_with_quality(
        path,
        initial_state,
        pull_request_url,
        "pass",
        "success-current",
        "head-sha-321",
    )
}

#[cfg(unix)]
fn write_listen_github_stub_with_checks(
    path: &Path,
    initial_state: &str,
    pull_request_url: &str,
    checks_mode: &str,
) -> Result<(), Box<dyn Error>> {
    write_listen_github_stub_with_quality(
        path,
        initial_state,
        pull_request_url,
        checks_mode,
        "success-current",
        "head-sha-321",
    )
}

#[cfg(unix)]
fn write_listen_github_stub_with_quality(
    path: &Path,
    initial_state: &str,
    pull_request_url: &str,
    checks_mode: &str,
    quality_mode: &str,
    head_sha: &str,
) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        format!(
            r#"#!/bin/sh
set -eu
log_file="$TEST_OUTPUT_DIR/gh.log"
state_file="$TEST_OUTPUT_DIR/gh-state.txt"
checks_file="$TEST_OUTPUT_DIR/gh-checks-count.txt"
if [ ! -f "$state_file" ]; then
  printf '%s' '{initial_state}' > "$state_file"
fi
state=$(cat "$state_file")
printf '%s\n' "$*" >> "$log_file"
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  case "$state" in
    draft)
      printf '%s' '[{{"number":321,"url":"{pull_request_url}","isDraft":true,"headRefName":"listen-branch","headRefOid":"{head_sha}"}}]'
      ;;
    stubborn-draft)
      printf '%s' '[{{"number":321,"url":"{pull_request_url}","isDraft":true,"headRefName":"listen-branch","headRefOid":"{head_sha}"}}]'
      ;;
    ready)
      printf '%s' '[{{"number":321,"url":"{pull_request_url}","isDraft":false,"headRefName":"listen-branch","headRefOid":"{head_sha}"}}]'
      ;;
    *)
      printf '%s' '[]'
      ;;
  esac
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "create" ]; then
  if printf '%s\n' "$*" | grep -q -- '--draft'; then
    printf '%s' 'draft' > "$state_file"
    printf '%s' '{{"number":321,"url":"{pull_request_url}","isDraft":true}}'
  else
    printf '%s' 'ready' > "$state_file"
    printf '%s' '{{"number":321,"url":"{pull_request_url}","isDraft":false}}'
  fi
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "ready" ]; then
  if [ "$state" = "stubborn-draft" ]; then
    printf '%s' 'stubborn-draft' > "$state_file"
  else
    printf '%s' 'ready' > "$state_file"
  fi
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  case "$state" in
    draft)
      printf '%s' '{{"number":321,"url":"{pull_request_url}","isDraft":true}}'
      ;;
    stubborn-draft)
      printf '%s' '{{"number":321,"url":"{pull_request_url}","isDraft":true}}'
      ;;
    ready)
      printf '%s' '{{"number":321,"url":"{pull_request_url}","isDraft":false}}'
      ;;
    *)
      printf 'unexpected gh invocation: %s\n' "$*" >&2
      exit 1
      ;;
  esac
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "checks" ]; then
  count=0
  exit_code=0
  if [ -f "$checks_file" ]; then
    count=$(cat "$checks_file")
  fi
  count=$((count + 1))
  printf '%s' "$count" > "$checks_file"
  case "{checks_mode}" in
    all-pass)
      printf '%s' '[{{"name":"ci / quality","state":"SUCCESS","bucket":"pass","description":"quality gate passed","link":"https://github.com/example/repo/actions/runs/1"}}]'
      ;;
    fail-once)
      if [ "$count" -eq 1 ]; then
        printf '%s' '[{{"name":"ci / quality","state":"FAILURE","bucket":"fail","description":"quality gate failed","link":"https://github.com/example/repo/actions/runs/1"}}]'
        exit_code=1
      else
        printf '%s' '[]'
      fi
      ;;
    fail-always)
      printf '%s' '[{{"name":"ci / quality","state":"FAILURE","bucket":"fail","description":"quality gate failed","link":"https://github.com/example/repo/actions/runs/1"}}]'
      exit_code=1
      ;;
    pending-then-pass)
      if [ "$count" -eq 1 ]; then
        printf '%s' '[{{"name":"ci / quality","state":"IN_PROGRESS","bucket":"pending","description":"quality gate still running","link":"https://github.com/example/repo/actions/runs/1"}}]'
        exit_code=8
      else
        printf '%s' '[{{"name":"ci / quality","state":"SUCCESS","bucket":"pass","description":"quality gate passed","link":"https://github.com/example/repo/actions/runs/1"}}]'
      fi
      ;;
    pending-then-fail)
      if [ "$count" -eq 1 ]; then
        printf '%s' '[{{"name":"ci / quality","state":"IN_PROGRESS","bucket":"pending","description":"quality gate still running","link":"https://github.com/example/repo/actions/runs/1"}}]'
        exit_code=8
      elif [ "$count" -eq 2 ]; then
        printf '%s' '[{{"name":"ci / quality","state":"FAILURE","bucket":"fail","description":"quality gate failed","link":"https://github.com/example/repo/actions/runs/1"}}]'
        exit_code=1
      else
        printf '%s' '[]'
      fi
      ;;
    pending-always)
      printf '%s' '[{{"name":"ci / quality","state":"IN_PROGRESS","bucket":"pending","description":"quality gate still running","link":"https://github.com/example/repo/actions/runs/1"}}]'
      exit_code=8
      ;;
    *)
      printf '%s' '[]'
      ;;
  esac
  exit "$exit_code"
fi
if [ "$1" = "run" ] && [ "$2" = "list" ]; then
  case "{quality_mode}" in
    success-current)
      printf '%s' '[{{"headSha":"{head_sha}","status":"completed","conclusion":"success","url":"https://github.com/example/repo/actions/runs/quality-current","workflowName":"quality"}}]'
      ;;
    success-old-sha)
      printf '%s' '[{{"headSha":"old-head-sha","status":"completed","conclusion":"success","url":"https://github.com/example/repo/actions/runs/quality-old","workflowName":"quality"}}]'
      ;;
    pending-current)
      printf '%s' '[{{"headSha":"{head_sha}","status":"in_progress","conclusion":null,"url":"https://github.com/example/repo/actions/runs/quality-pending","workflowName":"quality"}}]'
      ;;
    failure-current)
      printf '%s' '[{{"headSha":"{head_sha}","status":"completed","conclusion":"failure","url":"https://github.com/example/repo/actions/runs/quality-failed","workflowName":"quality"}}]'
      ;;
    missing)
      printf '%s' '[]'
      ;;
    error)
      printf '%s\n' 'gh run list failed' >&2
      exit 1
      ;;
    *)
      printf '%s\n' 'unexpected quality mode' >&2
      exit 1
      ;;
  esac
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "edit" ]; then
  exit 0
fi
if [ "$1" = "label" ] && [ "$2" = "create" ]; then
  exit 0
fi
printf 'unexpected gh invocation: %s\n' "$*" >&2
exit 1
"#
        ),
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn write_listen_github_stub_for_workspace_head(
    path: &Path,
    workspace: &Path,
    initial_state: &str,
    pull_request_url: &str,
) -> Result<(), Box<dyn Error>> {
    let head_sha = git_stdout(workspace, &["rev-parse", "HEAD"])?;
    write_listen_github_stub_with_quality(
        path,
        initial_state,
        pull_request_url,
        "pass",
        "success-current",
        head_sha.trim(),
    )
}

#[cfg(unix)]
fn write_listen_github_stub_with_checks_for_workspace_head(
    path: &Path,
    workspace: &Path,
    initial_state: &str,
    pull_request_url: &str,
    checks_mode: &str,
) -> Result<(), Box<dyn Error>> {
    let head_sha = git_stdout(workspace, &["rev-parse", "HEAD"])?;
    write_listen_github_stub_with_quality(
        path,
        initial_state,
        pull_request_url,
        checks_mode,
        "success-current",
        head_sha.trim(),
    )
}

#[cfg(unix)]
fn write_listen_store_session(
    config_path: &Path,
    repo_root: &Path,
    sessions: Vec<serde_json::Value>,
) -> Result<PathBuf, Box<dyn Error>> {
    let store_dir = listen_project_store_dir(config_path, repo_root, None)?;
    let source_root = listen_source_root(repo_root)?;
    let metastack_root = source_root.join(branding::PROJECT_DIR).canonicalize()?;
    fs::create_dir_all(store_dir.join("logs"))?;
    fs::write(
        store_dir.join("project.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "project_key": store_dir
                .file_name()
                .expect("store dir should have a file name")
                .to_string_lossy(),
            "project_label": source_root
                .file_name()
                .expect("source root should have a file name")
                .to_string_lossy(),
            "source_root": source_root.display().to_string(),
            "metastack_root": metastack_root.display().to_string()
        }))?,
    )?;
    let state_path = store_dir.join("session.json");
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "sessions": sessions
        }))?,
    )?;
    Ok(state_path)
}

#[cfg(unix)]
#[test]
fn listen_state_merge_rejects_stale_daemon_overwrite_of_enriched_worker_session()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues")
            .body_includes(r#""key":{"eq":"MET"}"#);
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-40",
                        "identifier": "MET-40",
                        "title": "Cross-actor session state",
                        "description": "Prevent stale daemon overwrites",
                        "url": "https://linear.app/issues/MET-40",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }]
                }
            }
        }));
    });

    init_repo_with_origin(&repo_root)?;
    let state_path = listen_state_path(&config_path, &repo_root)?;
    fs::create_dir_all(
        state_path
            .parent()
            .expect("listen state path should have a parent"),
    )?;

    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "sessions": [{
                "issue_id": "issue-40",
                "issue_identifier": "MET-40",
                "issue_title": "Cross-actor session state",
                "project_name": "MetaStack CLI",
                "team_key": "MET",
                "issue_url": "https://linear.app/issues/MET-40",
                "phase": "running",
                "summary": "Running",
                "brief_path": null,
                "backlog_issue_identifier": null,
                "backlog_issue_title": null,
                "backlog_path": null,
                "workspace_path": null,
                "branch": "met-40-state-owner",
                "pull_request": {},
                "workpad_comment_id": "comment-40",
                "updated_at_epoch_seconds": 1_773_575_600u64,
                "pid": 4242,
                "session_id": "daemon-session-40",
                "turns": 0,
                "tokens": {},
                "canonical": {},
                "turn_history": [],
                "log_path": format!("{}/agents/sessions/MET-40.log", branding::PROJECT_DIR)
            }]
        }))?,
    )?;

    let richer_state_path = stub_dir.join("richer-session.json");
    fs::write(
        &richer_state_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "sessions": [{
                "issue_id": "issue-40",
                "issue_identifier": "MET-40",
                "issue_title": "Cross-actor session state",
                "project_name": "MetaStack CLI",
                "team_key": "MET",
                "issue_url": "https://linear.app/issues/MET-40",
                "phase": "blocked",
                "summary": "Blocked | verification failed",
                "blocked": {
                    "category": "gate",
                    "reason": "verification failed and repair budget exhausted",
                    "retryable": false
                },
                "brief_path": null,
                "backlog_issue_identifier": null,
                "backlog_issue_title": null,
                "backlog_path": null,
                "workspace_path": null,
                "branch": "met-40-state-owner",
                "pull_request": {},
                "workpad_comment_id": "comment-40",
                "updated_at_epoch_seconds": 1_773_575_590u64,
                "pid": 4242,
                "session_id": "daemon-session-40",
                "latest_resume_handle": {
                    "provider": "claude",
                    "id": "resume-worker-40"
                },
                "turns": 2,
                "tokens": {
                    "input": 210,
                    "output": 34
                },
                "canonical": {
                    "provider": "claude",
                    "model": "sonnet",
                    "reasoning": "high",
                    "tokens": {
                        "input": 210,
                        "output": 34
                    }
                },
                "turn_history": [{
                    "turn": 1,
                    "prompt_mode": "full_prompt",
                    "tokens": {
                        "input": 210,
                        "output": 34
                    },
                    "captured_at_epoch_seconds": 1_773_575_590u64
                }, {
                    "turn": 2,
                    "prompt_mode": "continuation",
                    "tokens": {
                        "input": 80,
                        "output": 13
                    },
                    "captured_at_epoch_seconds": 1_773_575_600u64
                }],
                "log_path": format!("{}/agents/sessions/MET-40.log", branding::PROJECT_DIR)
            }]
        }))?,
    )?;

    let writer_state_path = state_path.clone();
    let writer_richer_state_path = richer_state_path.clone();
    let writer = thread::spawn(move || {
        for _ in 0..3 {
            thread::sleep(Duration::from_millis(150));
            fs::copy(&writer_richer_state_path, &writer_state_path)
                .expect("state rewrite should succeed");
        }
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .args([
            "listen",
            "--render-once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success();
    writer.join().expect("state rewrite thread should join");

    let state_text = fs::read_to_string(&state_path)?;
    let state: serde_json::Value = serde_json::from_str(&state_text)?;
    let session = &state["sessions"][0];
    assert_eq!(session["phase"], json!("blocked"), "state={state_text}");
    assert_eq!(
        session["blocked"]["reason"],
        json!("verification failed and repair budget exhausted")
    );
    assert_eq!(session["summary"], json!("Blocked | verification failed"));
    assert_eq!(session["canonical"]["provider"], json!("claude"));
    assert_eq!(session["canonical"]["model"], json!("sonnet"));
    assert_eq!(session["canonical"]["reasoning"], json!("high"));
    assert_eq!(session["canonical"]["tokens"]["input"], json!(210));
    assert_eq!(session["canonical"]["tokens"]["output"], json!(34));
    assert_eq!(
        session["latest_resume_handle"]["id"],
        json!("resume-worker-40")
    );
    assert_eq!(
        session["turn_history"].as_array().map(Vec::len),
        Some(2),
        "state={state}"
    );

    assert!(viewer_mock.calls() <= 1);
    assert!(issues_mock.calls() >= 1);

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_verification_requires_quality_workflow_success_for_exact_pr_head_sha()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();

    struct Case {
        name: &'static str,
        pr_state: &'static str,
        pr_head_sha: Option<&'static str>,
        quality_mode: &'static str,
        expect_workflow_lookup: bool,
        expected_report_status: &'static str,
        expected_phase: &'static str,
        expected_snippet: &'static str,
    }

    fn run_case(case: &Case) -> Result<(String, String, String, String), Box<dyn Error>> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let config_path = temp.path().join("metastack.toml");
        let bin_dir = temp.path().join("bin");
        let stub_dir = temp.path().join("stub-output");
        let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
        let api_url = server.url.clone();
        fs::create_dir_all(&repo_root)?;
        fs::create_dir_all(&bin_dir)?;
        fs::create_dir_all(&stub_dir)?;

        write_minimal_planning_context(
            &repo_root,
            r#"{
  "linear": {
    "team": "MET"
  },
  "validation": {
    "commands": ["true"],
    "repair_attempts": 0
  }
}
"#,
        )?;
        write_onboarded_config(
            &config_path,
            format!(
                r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "exec-stub"

[agents.routing.commands."agents.listen.verification"]
provider = "verify-stub"

[agents.commands.exec-stub]
command = "exec-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.verify-stub]
command = "verify-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
battle_test_count = 0
"#,
            ),
        )?;

        fs::write(
            bin_dir.join("exec-stub"),
            r#"#!/bin/sh
set -eu
printf '%s' "$1" > "$TEST_OUTPUT_DIR/exec-prompt.txt"
mkdir -p src
printf '// verification quality gate\n' > src/quality_gate.rs
"#,
        )?;
        fs::write(
            bin_dir.join("verify-stub"),
            r#"#!/bin/sh
set -eu
printf '%s' "$1" > "$TEST_OUTPUT_DIR/verify-prompt.txt"
printf '%s' '{"summary":"Verifier approved the branch","criteria":[{"name":"Verification proof.","status":"passed","summary":"Verification proof is satisfied."}],"battle_tests":[],"notes":["verifier passed"]}'
"#,
        )?;
        for command_name in ["exec-stub", "verify-stub"] {
            let mut permissions = fs::metadata(bin_dir.join(command_name))?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(bin_dir.join(command_name), permissions)?;
        }

        init_repo_with_origin(&repo_root)?;
        let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
        let branch = if case.pr_state == "none" {
            "main"
        } else {
            "met-32-quality-gate"
        };
        ProcessCommand::new("git")
            .args([
                "-C",
                workspace.to_str().expect("utf8"),
                "checkout",
                "-B",
                branch,
                "main",
            ])
            .status()?;
        fs::write(workspace.join("src.rs"), "pub fn quality_gate() {}\n")?;
        ProcessCommand::new("git")
            .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
            .status()?;
        ProcessCommand::new("git")
            .args([
                "-C",
                workspace.to_str().expect("utf8"),
                "commit",
                "-m",
                "Seed quality gate proof",
            ])
            .status()?;
        let head_sha = git_stdout(workspace.as_path(), &["rev-parse", "HEAD"])?;
        let pull_request_head_sha = case.pr_head_sha.unwrap_or(head_sha.trim());
        write_listen_github_stub_with_quality(
            &bin_dir.join("gh"),
            case.pr_state,
            "https://github.com/example/repo/pull/321",
            "pass",
            case.quality_mode,
            pull_request_head_sha,
        )?;

        let recipe_dir = workspace.join(format!("{}/verification/recipes", branding::PROJECT_DIR));
        fs::create_dir_all(&recipe_dir)?;
        fs::write(
            recipe_dir.join("agents.listen.yaml"),
            r#"quality_criteria:
  - Verification proof.
"#,
        )?;
        let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
        fs::create_dir_all(&backlog_dir)?;
        fs::write(
            backlog_dir.join("index.md"),
            "# MET-32\n\n## Tasks\n\n- [x] Verification ready\n",
        )?;

        let current_path = std::env::var("PATH")?;
        meta()
            .current_dir(&repo_root)
            .env("METASTACK_CONFIG", &config_path)
            .env("TEST_OUTPUT_DIR", &stub_dir)
            .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
            .args([
                "listen-worker",
                "--source-root",
                repo_root.to_str().expect("utf8"),
                "--workspace",
                workspace.to_str().expect("utf8"),
                "--issue",
                "MET-32",
                "--workpad-comment-id",
                "comment-32",
                "--backlog-issue",
                "MET-32",
                "--api-key",
                "token",
                "--api-url",
                &api_url,
                "--max-turns",
                "1",
            ])
            .assert()
            .success();

        let verification_path = listen_verification_json_path(&config_path, &repo_root, "MET-32")?;
        wait_for_path(&verification_path)?;
        let report_text = fs::read_to_string(&verification_path)?;
        let report: serde_json::Value = serde_json::from_str(&report_text)?;
        let state_text = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
        let gh_log = fs::read_to_string(stub_dir.join("gh.log")).unwrap_or_default();

        Ok((
            report["status"].as_str().unwrap_or_default().to_string(),
            report_text,
            state_text,
            gh_log,
        ))
    }

    let cases = [
        Case {
            name: "success-current",
            pr_state: "draft",
            pr_head_sha: None,
            quality_mode: "success-current",
            expect_workflow_lookup: true,
            expected_report_status: "passed",
            expected_phase: "completed",
            expected_snippet: "`quality` passed for PR #321 head",
        },
        Case {
            name: "stale-green",
            pr_state: "draft",
            pr_head_sha: None,
            quality_mode: "success-old-sha",
            expect_workflow_lookup: true,
            expected_report_status: "failed",
            expected_phase: "blocked",
            expected_snippet: "workflow metadata did not match PR #321 head",
        },
        Case {
            name: "local-head-mismatch",
            pr_state: "draft",
            pr_head_sha: Some("remote-head-sha"),
            quality_mode: "success-current",
            expect_workflow_lookup: false,
            expected_report_status: "failed",
            expected_phase: "blocked",
            expected_snippet: "Local workspace HEAD",
        },
        Case {
            name: "pending-current",
            pr_state: "draft",
            pr_head_sha: None,
            quality_mode: "pending-current",
            expect_workflow_lookup: true,
            expected_report_status: "failed",
            expected_phase: "blocked",
            expected_snippet: "is still `in_progress`",
        },
        Case {
            name: "failed-current",
            pr_state: "draft",
            pr_head_sha: None,
            quality_mode: "failure-current",
            expect_workflow_lookup: true,
            expected_report_status: "failed",
            expected_phase: "blocked",
            expected_snippet: "concluded `failure`",
        },
        Case {
            name: "missing-run",
            pr_state: "draft",
            pr_head_sha: None,
            quality_mode: "missing",
            expect_workflow_lookup: true,
            expected_report_status: "failed",
            expected_phase: "blocked",
            expected_snippet: "No `quality` workflow run was found",
        },
        Case {
            name: "no-pr",
            pr_state: "none",
            pr_head_sha: None,
            quality_mode: "success-current",
            expect_workflow_lookup: false,
            expected_report_status: "failed",
            expected_phase: "blocked",
            expected_snippet: "No open branch PR matched",
        },
        Case {
            name: "workflow-error",
            pr_state: "draft",
            pr_head_sha: None,
            quality_mode: "error",
            expect_workflow_lookup: true,
            expected_report_status: "failed",
            expected_phase: "blocked",
            expected_snippet: "Could not resolve `quality` workflow metadata",
        },
    ];

    for case in &cases {
        let (status, report_text, state_text, gh_log) = run_case(case)?;
        assert_eq!(
            status, case.expected_report_status,
            "case={} report={report_text}",
            case.name
        );
        assert!(
            report_text.contains(EXACT_SHA_QUALITY_CRITERION),
            "case={} report={report_text}",
            case.name
        );
        assert!(
            report_text.contains(case.expected_snippet),
            "case={} report={report_text}",
            case.name
        );
        assert!(
            report_text.contains("quality"),
            "case={} report={report_text}",
            case.name
        );
        assert!(
            state_text.contains(&format!("\"phase\": \"{}\"", case.expected_phase)),
            "case={} state={state_text}",
            case.name
        );
        let expected_branch = if case.pr_state == "none" {
            "main"
        } else {
            "met-32-quality-gate"
        };
        assert!(
            gh_log.contains(&format!(
                "pr list --state open --head {expected_branch} --base main --json number,url,isDraft,headRefName,headRefOid"
            )),
            "case={} gh_log={gh_log}",
            case.name
        );
        if case.expect_workflow_lookup {
            assert!(
                gh_log.contains("run list --commit"),
                "case={} gh_log={gh_log}",
                case.name
            );
        } else {
            assert!(
                !gh_log.contains("run list --commit"),
                "case={} gh_log={gh_log}",
                case.name
            );
        }
    }

    Ok(())
}

#[cfg(unix)]
fn listen_detail_path(
    config_path: &Path,
    repo_root: &Path,
    issue_identifier: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    Ok(listen_project_store_dir(config_path, repo_root, None)?
        .join("session-details")
        .join(format!("{issue_identifier}.json")))
}

#[cfg(unix)]
fn listen_verification_json_path(
    config_path: &Path,
    repo_root: &Path,
    issue_identifier: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    Ok(listen_project_store_dir(config_path, repo_root, None)?
        .join("verification")
        .join(format!("{issue_identifier}.json")))
}

#[cfg(unix)]
fn listen_verification_markdown_path(
    config_path: &Path,
    repo_root: &Path,
    issue_identifier: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    Ok(listen_project_store_dir(config_path, repo_root, None)?
        .join("verification")
        .join(format!("{issue_identifier}.md")))
}

#[cfg(unix)]
fn write_listen_verification_report(
    config_path: &Path,
    repo_root: &Path,
    issue_identifier: &str,
    report: serde_json::Value,
) -> Result<PathBuf, Box<dyn Error>> {
    let path = listen_verification_json_path(config_path, repo_root, issue_identifier)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    Ok(path)
}

#[cfg(unix)]
fn read_listen_fixture(name: &str) -> Result<String, Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("listen")
        .join(name);
    Ok(fs::read_to_string(path)?)
}

#[cfg(unix)]
fn listen_session_json(
    issue_identifier: &str,
    phase: &str,
    updated_at_epoch_seconds: u64,
    pid: Option<u32>,
) -> serde_json::Value {
    json!({
        "issue_id": format!("{issue_identifier}-id"),
        "issue_identifier": issue_identifier,
        "issue_title": format!("{issue_identifier} title"),
        "project_name": "MetaStack CLI",
        "team_key": "MET",
        "issue_url": format!("https://linear.app/issues/{issue_identifier}"),
        "phase": phase,
        "summary": format!("{issue_identifier} summary"),
        "brief_path": format!("{}/agents/briefs/{issue_identifier}.md", branding::PROJECT_DIR),
        "workspace_path": format!("/tmp/{issue_identifier}"),
        "workpad_comment_id": format!("comment-{issue_identifier}"),
        "updated_at_epoch_seconds": updated_at_epoch_seconds,
        "pid": pid,
        "session_id": format!("session-{issue_identifier}"),
        "turns": 1,
        "tokens": {},
        "log_path": format!("logs/{issue_identifier}.log")
    })
}

#[cfg(unix)]
fn closed_graphql_url() -> Result<String, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    Ok(format!("http://{address}/graphql"))
}

#[cfg(unix)]
fn prepare_listen_repo_with_existing_session(
    repo_root: &Path,
    config_path: &Path,
    api_url: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    fs::create_dir_all(repo_root)?;
    write_minimal_planning_context(
        repo_root,
        r#"{
  "linear": {
    "team": "ENG",
    "project_id": "project-1"
  },
  "listen": {
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    init_repo_with_origin(repo_root)?;
    write_listen_store_session(
        config_path,
        repo_root,
        vec![listen_session_json(
            "ENG-10181",
            "blocked",
            1_773_575_100,
            None,
        )],
    )
}

#[cfg(unix)]
type PostPublicationCiFixture = (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf);

#[cfg(unix)]
fn prepare_post_publication_ci_fixture(
    temp_root: &Path,
    api_url: &str,
    planning_context: &str,
    config_extra: &str,
    agent_script: &str,
    checks_mode: &str,
    branch: &str,
) -> Result<PostPublicationCiFixture, Box<dyn Error>> {
    let repo_root = temp_root.join("repo");
    let config_path = temp_root.join("metastack.toml");
    let bin_dir = temp_root.join("bin");
    let stub_dir = temp_root.join("stub-output");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(&repo_root, planning_context)?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
{config_extra}
"#,
        ),
    )?;
    fs::write(bin_dir.join("agent-stub"), agent_script)?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    write_minimal_planning_context(&workspace, planning_context)?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn ready() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare CI settle proof",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_with_checks_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "none",
        "https://github.com/example/repo/pull/321",
        checks_mode,
    )?;
    fs::write(
        workspace.join("dirty-skip.txt"),
        "keep workspace for assertions\n",
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    Ok((repo_root, config_path, bin_dir, stub_dir, workspace))
}

#[cfg(unix)]
fn run_listen_worker_fixture(
    repo_root: &Path,
    config_path: &Path,
    bin_dir: &Path,
    stub_dir: &Path,
    workspace: &Path,
    api_url: &str,
    max_turns: u32,
) -> Result<(), Box<dyn Error>> {
    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(repo_root)
        .env("METASTACK_CONFIG", config_path)
        .env("TEST_OUTPUT_DIR", stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--api-key",
            "token",
            "--api-url",
            api_url,
            "--max-turns",
            &max_turns.to_string(),
        ])
        .assert()
        .success();
    Ok(())
}

#[cfg(unix)]
fn inspect_listen_sessions(repo_root: &Path, config_path: &Path) -> Result<String, Box<dyn Error>> {
    let output = meta()
        .current_dir(repo_root)
        .env("METASTACK_CONFIG", config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("utf8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    Ok(String::from_utf8(output)?)
}

#[cfg(unix)]
fn render_listen_dashboard_once(
    repo_root: &Path,
    config_path: &Path,
    bin_dir: &Path,
    stub_dir: &Path,
) -> Result<String, Box<dyn Error>> {
    let current_path = std::env::var("PATH")?;
    let output = meta()
        .current_dir(repo_root)
        .env("METASTACK_CONFIG", config_path)
        .env("TEST_OUTPUT_DIR", stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("utf8"),
            "--render-once",
            "--width",
            "140",
            "--height",
            "36",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    Ok(String::from_utf8(output)?)
}

#[test]
fn listen_requires_auth_when_not_in_demo_mode() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir().expect("tempdir should build");
    let config_path = temp.path().join("metastack.toml");
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        temp.path(),
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  }
}
"#,
    )?;

    meta()
        .current_dir(temp.path())
        .env_remove("LINEAR_API_KEY")
        .env("METASTACK_CONFIG", &config_path)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HOME")
        .arg("listen")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("LINEAR_API_KEY")
                .or(predicate::str::contains("Linear profile")
                    .and(predicate::str::contains("is not configured"))),
        );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_clear_issue_identifier_removes_only_matching_session()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![
            listen_session_json("ENG-10163", "blocked", 200, None),
            listen_session_json("ENG-10164", "blocked", 300, None),
        ],
    )?;
    fs::write(
        listen_log_path(&config_path, &repo_root, "ENG-10163")?,
        "log 63\n",
    )?;
    fs::write(
        listen_log_path(&config_path, &repo_root, "ENG-10164")?,
        "log 64\n",
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "clear",
            "ENG-10163",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Cleared 1 stored MetaListen session(s) matched by issue `ENG-10163`",
        ))
        .stdout(predicate::str::contains("ENG-10163 [Blocked]"));

    let state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let sessions = state["sessions"]
        .as_array()
        .expect("sessions should remain an array");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["issue_identifier"], "ENG-10164");
    assert!(listen_log_path(&config_path, &repo_root, "ENG-10164")?.is_file());

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ENG-10164 [Blocked]"))
        .stdout(predicate::str::contains("ENG-10163").not());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_clear_refuses_live_pid_records() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let mut child = ProcessCommand::new("sleep").arg("30").spawn()?;
    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![listen_session_json(
            "ENG-10163",
            "running",
            300,
            Some(child.id()),
        )],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "clear",
            "--all",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cannot clear live MetaListen session record(s)",
        ))
        .stderr(predicate::str::contains("ENG-10163"))
        .stderr(predicate::str::contains("pid"));

    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_listen_sessions_clear_blocked_preserves_other_selector_states()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![
            listen_session_json("ENG-10163", "blocked", now, None),
            listen_session_json("ENG-10164", "completed", now, None),
            listen_session_json("ENG-10165", "running", now, None),
        ],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "sessions",
            "clear",
            "--blocked",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Cleared 1 stored MetaListen session(s) matched by `--blocked`",
        ))
        .stdout(predicate::str::contains("ENG-10163 [Blocked]"));

    let state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let sessions = state["sessions"]
        .as_array()
        .expect("sessions should remain an array");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0]["issue_identifier"], "ENG-10164");
    assert_eq!(sessions[1]["issue_identifier"], "ENG-10165");

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ENG-10164 [Completed]"))
        .stdout(predicate::str::contains("ENG-10165 [Running]"))
        .stdout(predicate::str::contains("ENG-10163").not());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_clear_completed_preserves_blocked_records() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![
            listen_session_json("ENG-10163", "completed", now, None),
            listen_session_json("ENG-10164", "blocked", now, None),
        ],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "clear",
            "--completed",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Cleared 1 stored MetaListen session(s) matched by `--completed`",
        ))
        .stdout(predicate::str::contains("ENG-10163 [Completed]"));

    let state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let sessions = state["sessions"]
        .as_array()
        .expect("sessions should remain an array");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["issue_identifier"], "ENG-10164");

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_clear_stale_removes_only_dead_pid_records() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let mut child = ProcessCommand::new("sleep").arg("30").spawn()?;
    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![
            listen_session_json("ENG-10163", "blocked", 100, Some(99_999)),
            listen_session_json("ENG-10164", "running", 200, Some(child.id())),
        ],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "clear",
            "--stale",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Cleared 1 stored MetaListen session(s) matched by `--stale`",
        ))
        .stdout(predicate::str::contains("ENG-10163 [Blocked]"));

    let state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let sessions = state["sessions"]
        .as_array()
        .expect("sessions should remain an array");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["issue_identifier"], "ENG-10164");

    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_list_prunes_expired_completed_sessions_on_load() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![
            listen_session_json("ENG-10163", "completed", now - (24 * 60 * 60) - 1, None),
            listen_session_json("ENG-10164", "blocked", now, None),
        ],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ENG-10164"))
        .stdout(predicate::str::contains("ENG-10163").not());

    let state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let sessions = state["sessions"]
        .as_array()
        .expect("sessions should remain an array");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["issue_identifier"], "ENG-10164");
    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_inspect_prunes_expired_completed_sessions_on_load() -> Result<(), Box<dyn Error>>
{
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![
            listen_session_json("ENG-10163", "completed", now - (24 * 60 * 60) - 1, None),
            listen_session_json("ENG-10164", "blocked", now, None),
        ],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Tracked sessions:"))
        .stdout(predicate::str::contains("ENG-10164 [Blocked]"))
        .stdout(predicate::str::contains("ENG-10163").not());

    let state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let sessions = state["sessions"]
        .as_array()
        .expect("sessions should remain an array");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["issue_identifier"], "ENG-10164");
    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_list_and_inspect_surface_resume_and_token_metadata() -> Result<(), Box<dyn Error>>
{
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![
            json!({
                "issue_id": "issue-10181",
                "issue_identifier": "ENG-10181",
                "issue_title": "Track built-in listen token usage",
                "project_name": "MetaStack CLI",
                "team_key": "MET",
                "issue_url": "https://linear.app/issues/ENG-10181",
                "phase": "running",
                "summary": "Token telemetry is flowing",
                "brief_path": null,
                "workspace_path": "/tmp/ENG-10181",
                "workpad_comment_id": "comment-10181",
                "updated_at_epoch_seconds": 1_773_575_100u64,
                "pid": 4242,
                "session_id": "session-10181",
                "latest_resume_handle": {
                    "provider": "codex",
                    "id": "019d0763-afc9-70d1-8022-51624918cf76"
                },
                "canonical": {
                    "provider": "claude"
                },
                "turns": 2,
                "tokens": {
                    "input": 210,
                    "output": 34
                },
                "log_path": "logs/ENG-10181.log"
            }),
            json!({
                "issue_id": "issue-10182",
                "issue_identifier": "ENG-10182",
                "issue_title": "Fallback when usage is unavailable",
                "project_name": "MetaStack CLI",
                "team_key": "MET",
                "issue_url": "https://linear.app/issues/ENG-10182",
                "phase": "blocked",
                "summary": "Provider did not emit exact counts",
                "brief_path": null,
                "workspace_path": "/tmp/ENG-10182",
                "workpad_comment_id": "comment-10182",
                "updated_at_epoch_seconds": 1_773_575_000u64,
                "pid": null,
                "session_id": "session-10182",
                "turns": 1,
                "tokens": {},
                "log_path": "logs/ENG-10182.log"
            }),
        ],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PROVIDER"))
        .stdout(predicate::str::contains("RESUME ID"))
        .stdout(predicate::str::contains("claude"))
        .stdout(predicate::str::contains(
            "019d0763-afc9-70d1-8022-51624918cf76",
        ));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Latest session:"))
        .stdout(predicate::str::contains("Detail file:"))
        .stdout(predicate::str::contains("Provider: claude"))
        .stdout(predicate::str::contains("  - Tokens: in 210 | out 34 | total 244"))
        .stdout(predicate::str::contains("Resume provider: codex"))
        .stdout(predicate::str::contains(
            "Resume ID: 019d0763-afc9-70d1-8022-51624918cf76",
        ))
        .stdout(predicate::str::contains(
            "  - ENG-10181 [Running] Token telemetry is flowing | tokens in 210 | out 34 | total 244",
        ))
        .stdout(predicate::str::contains(
            "  - ENG-10182 [Blocked] Provider did not emit exact counts | tokens n/a",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_list_and_inspect_show_explicit_unavailable_resume_metadata()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "issue-10183",
            "issue_identifier": "ENG-10183",
            "issue_title": "Resume metadata missing",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/ENG-10183",
            "phase": "blocked",
            "summary": "Waiting on a retry",
            "brief_path": null,
            "workspace_path": "/tmp/ENG-10183",
            "workpad_comment_id": "comment-10183",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": "legacy-session-should-not-surface",
            "turns": 2,
            "tokens": {},
            "log_path": "logs/ENG-10183.log"
        })],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unavailable"))
        .stdout(predicate::str::contains("legacy-session-should-not-surface").not());

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Resume provider: unavailable"))
        .stdout(predicate::str::contains("Resume ID: unavailable"))
        .stdout(predicate::str::contains("legacy-session-should-not-surface").not());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_list_and_inspect_keep_legacy_blocked_fallback_without_metadata()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let mut session = listen_session_json("ENG-10184", "blocked", 1_773_575_100, None);
    session["summary"] = json!("Blocked | workspace missing");
    write_listen_store_session(&config_path, &repo_root, vec![session])?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Blocked"))
        .stdout(predicate::str::contains("Setup Err").not());

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Phase: Blocked"))
        .stdout(predicate::str::contains("Blocked category").not());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_list_and_inspect_surface_structured_blocked_category()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let mut session = listen_session_json("ENG-10185", "blocked", 1_773_575_100, None);
    session["summary"] = json!("Blocked | workspace missing");
    session["blocked"] = json!({
        "category": "setup",
        "reason": "workspace missing",
        "retryable": false
    });
    write_listen_store_session(&config_path, &repo_root, vec![session])?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Setup Err"));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Phase: Setup Err"))
        .stdout(predicate::str::contains("Blocked category: Setup"))
        .stdout(predicate::str::contains("Blocked retryable: no"))
        .stdout(predicate::str::contains("ENG-10185 [Setup Err]"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_inspect_surfaces_structured_detail_fields() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "ENG-10181-id",
            "issue_identifier": "ENG-10181",
            "issue_title": "ENG-10181 title",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/ENG-10181",
            "phase": "running",
            "summary": "Token telemetry is flowing",
            "brief_path": format!("{}/agents/briefs/ENG-10181.md", branding::PROJECT_DIR),
            "backlog_path": format!("{}/backlog/ENG-10181", branding::PROJECT_DIR),
            "workspace_path": "/tmp/ENG-10181",
            "branch": "met-27-detail",
            "pull_request": {
                "number": 321,
                "url": "https://github.com/metastack-labs/metastack-cli/pull/321",
                "status": "draft"
            },
            "workpad_comment_id": "comment-10181",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": "session-10181",
            "turns": 4,
            "tokens": {
                "input": 210,
                "output": 34
            },
            "log_path": "logs/ENG-10181.log"
        })],
    )?;

    let detail_path = listen_detail_path(&config_path, &repo_root, "ENG-10181")?;
    fs::create_dir_all(
        detail_path
            .parent()
            .expect("detail path should have a parent"),
    )?;
    fs::write(
        &detail_path,
        serde_json::to_vec_pretty(&json!({
            "version": 3,
            "issue_identifier": "ENG-10181",
            "issue_title": "ENG-10181 title",
            "updated_at_epoch_seconds": 1_773_575_120u64,
            "session_updated_at_epoch_seconds": 1_773_575_100u64,
            "phase": "running",
            "summary": "Token telemetry is flowing",
            "turns": 4,
            "tokens": {
                "input": 210,
                "output": 34
            },
            "turn_history": [
                {
                    "turn": 1,
                    "prompt_mode": "full_prompt",
                    "tokens": {
                        "input": 210,
                        "output": 34
                    },
                    "captured_at_epoch_seconds": 1_773_575_010u64
                },
                {
                    "turn": 2,
                    "prompt_mode": "continuation",
                    "tokens": {
                        "input": 80,
                        "output": 13
                    },
                    "captured_at_epoch_seconds": 1_773_575_050u64
                }
            ],
            "pull_request": {
                "number": 321,
                "url": "https://github.com/metastack-labs/metastack-cli/pull/321",
                "status": "draft"
            },
            "references": {
                "workspace_path": "/tmp/ENG-10181",
                "backlog_path": format!("{}/backlog/ENG-10181", branding::PROJECT_DIR),
                "brief_path": format!("{}/agents/briefs/ENG-10181.md", branding::PROJECT_DIR),
                "workpad_comment_id": "comment-10181",
                "log_path": "logs/ENG-10181.log",
                "branch": "met-27-detail"
            },
            "prompt_context": [
                {
                    "label": "Brief",
                    "value": format!("{}/agents/briefs/ENG-10181.md", branding::PROJECT_DIR)
                },
                {
                    "label": "Backlog index",
                    "value": format!("{}/backlog/ENG-10181/index.md", branding::PROJECT_DIR)
                }
            ],
            "milestones": [
                {
                    "at_epoch_seconds": 1_773_575_000u64,
                    "phase": "claimed",
                    "summary": "Claimed ticket",
                    "turns": 1,
                    "pull_request_status": "unpublished",
                    "pull_request_number": null
                },
                {
                    "at_epoch_seconds": 1_773_575_050u64,
                    "phase": "running",
                    "summary": "Opened draft PR",
                    "turns": 3,
                    "pull_request_status": "draft",
                    "pull_request_number": 321
                }
            ],
            "log_excerpts": [
                {
                    "line_number": 19,
                    "text": "worker boot complete"
                },
                {
                    "line_number": 27,
                    "text": "published draft PR"
                }
            ]
        }))?,
    )?;
    fs::write(
        listen_log_path(&config_path, &repo_root, "ENG-10181")?,
        "{\"message\":\"worker boot complete\"}\n{\"message\":\"published draft PR\"}\n",
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Detail status: available"))
        .stdout(predicate::str::contains("Detail PR: draft #321"))
        .stdout(predicate::str::contains(
            "Detail PR URL: https://github.com/metastack-labs/metastack-cli/pull/321",
        ))
        .stdout(predicate::str::contains("Detail branch: met-27-detail"))
        .stdout(predicate::str::contains("Detail workpad: comment-10181"))
        .stdout(predicate::str::contains("Prompt context:"))
        .stdout(predicate::str::contains(format!(
            "Brief: {}/agents/briefs/ENG-10181.md",
            branding::PROJECT_DIR
        )))
        .stdout(predicate::str::contains("Recent milestones:"))
        .stdout(predicate::str::contains(
            "Running: Opened draft PR | turns 3 | draft #321",
        ))
        .stdout(predicate::str::contains("Turn history:").not());

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--turns",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Turn history:"))
        .stdout(predicate::str::contains(
            "turn 1 tokens: in 210 | out 34 | prompt_mode=full_prompt",
        ))
        .stdout(predicate::str::contains(
            "turn 2 tokens: in 80 | out 13 | prompt_mode=continuation",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_inspect_repairs_canonical_metadata_from_mixed_historical_log()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "ENG-10184-id",
            "issue_identifier": "ENG-10184",
            "issue_title": "Repair canonical state from mixed historical log",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/ENG-10184",
            "phase": "blocked",
            "summary": "Historical repair needed",
            "brief_path": null,
            "workspace_path": "/tmp/ENG-10184",
            "workpad_comment_id": "comment-10184",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": "session-10184",
            "turns": 2,
            "tokens": {},
            "log_path": "logs/ENG-10184.log"
        })],
    )?;
    fs::write(
        listen_log_path(&config_path, &repo_root, "ENG-10184")?,
        read_listen_fixture("mixed-legacy-and-branded.log")?,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "  - Tokens: in 290 | out 47 | total 337",
        ))
        .stdout(predicate::str::contains("  - Provider: claude"))
        .stdout(predicate::str::contains("  - Model: sonnet"))
        .stdout(predicate::str::contains("  - Reasoning: high"))
        .stdout(predicate::str::contains("  - Detail provider: claude"))
        .stdout(predicate::str::contains("  - Detail model: sonnet"))
        .stdout(predicate::str::contains("  - Detail reasoning: high"))
        .stdout(predicate::str::contains(
            "  - Detail tokens: in 290 | out 47 | total 337",
        ));

    let state: serde_json::Value = serde_json::from_str(&fs::read_to_string(listen_state_path(
        &config_path,
        &repo_root,
    )?)?)?;
    let detail: serde_json::Value = serde_json::from_str(&fs::read_to_string(
        listen_detail_path(&config_path, &repo_root, "ENG-10184")?,
    )?)?;
    assert_eq!(
        state.pointer("/sessions/0/canonical/provider"),
        Some(&json!("claude"))
    );
    assert_eq!(
        state.pointer("/sessions/0/canonical/model"),
        Some(&json!("sonnet"))
    );
    assert_eq!(
        state.pointer("/sessions/0/canonical/reasoning"),
        Some(&json!("high"))
    );
    assert_eq!(
        state.pointer("/sessions/0/canonical/tokens/input"),
        Some(&json!(290))
    );
    assert_eq!(
        state.pointer("/sessions/0/canonical/tokens/output"),
        Some(&json!(47))
    );
    assert_eq!(
        detail.pointer("/canonical/provider"),
        Some(&json!("claude"))
    );
    assert_eq!(detail.pointer("/canonical/model"), Some(&json!("sonnet")));
    assert_eq!(detail.pointer("/canonical/reasoning"), Some(&json!("high")));
    assert_eq!(detail.pointer("/canonical/tokens/input"), Some(&json!(290)));
    assert_eq!(detail.pointer("/canonical/tokens/output"), Some(&json!(47)));

    Ok(())
}

#[test]
fn listen_once_demo_outputs_terminal_summary_without_browser_endpoints()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{} listen",
            branding::COMMAND_NAME
        )))
        .stdout(predicate::str::contains("Watching: all assignees"))
        .stdout(predicate::str::contains("Dashboard: terminal summary"))
        .stdout(predicate::str::contains("http://").not())
        .stdout(predicate::str::contains("127.0.0.1").not())
        .stdout(predicate::str::contains("localhost").not())
        .stdout(predicate::str::contains("(execute-origin)"))
        .stdout(predicate::str::contains("MET-17"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_summary_reports_route_resolved_execution_agent() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "global-stub"

[agents.routing.commands."agents.listen"]
provider = "listen-stub"

[agents.commands.global-stub]
command = "global-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.listen-stub]
command = "listen-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;

    for command_name in ["global-stub", "listen-stub"] {
        let path = bin_dir.join(command_name);
        fs::write(
            &path,
            format!(
                r#"#!/bin/sh
printf '%s' "{command_name}" > /dev/null
"#
            ),
        )?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions)?;
    }

    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": []
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "agents",
            "listen",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dashboard: terminal summary"))
        .stdout(predicate::str::contains("Execution agent: listen-stub"))
        .stdout(predicate::str::contains("Execution agent: global-stub").not());

    assert!(viewer_mock.calls() >= 1);
    assert!(issues_mock.calls() >= 1);

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_summary_prefers_explicit_execution_agent_override() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "global-stub"

[agents.routing.commands."agents.listen"]
provider = "listen-stub"

[agents.commands.global-stub]
command = "global-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.listen-stub]
command = "listen-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;

    for command_name in ["global-stub", "listen-stub"] {
        let path = bin_dir.join(command_name);
        fs::write(
            &path,
            format!(
                r#"#!/bin/sh
printf '%s' "{command_name}" > /dev/null
"#
            ),
        )?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions)?;
    }

    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": []
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "agents",
            "listen",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--agent",
            "global-stub",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dashboard: terminal summary"))
        .stdout(predicate::str::contains("Execution agent: global-stub"))
        .stdout(predicate::str::contains("Execution agent: listen-stub").not());

    assert!(viewer_mock.calls() >= 1);
    assert!(issues_mock.calls() >= 1);

    Ok(())
}

#[test]
fn listen_sessions_inspect_surfaces_detail_pr_ref_without_url() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let issue_identifier = "ENG-10182";
    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": format!("{issue_identifier}-id"),
            "issue_identifier": issue_identifier,
            "issue_title": "Investigate session detail PR ref",
            "project_name": "MetaStack CLI",
            "team_key": "ENG",
            "issue_url": "https://linear.app/metastack-labs/issue/ENG-10182",
            "phase": "running",
            "summary": "Structured detail keeps PR number only",
            "brief_path": format!("{}/agents/briefs/{issue_identifier}.md", branding::PROJECT_DIR),
            "backlog_issue_identifier": issue_identifier,
            "workspace_path": format!("/tmp/{issue_identifier}"),
            "workpad_comment_id": format!("comment-{issue_identifier}"),
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "session_id": "codex-session-10182",
            "started_at_epoch_seconds": 1_773_575_000u64,
            "turns": 2,
            "tokens": {
                "input": 55,
                "output": 13
            },
            "pull_request": {
                "number": 482,
                "status": "draft"
            },
            "log_path": format!("logs/{issue_identifier}.log")
        })],
    )?;

    let detail_path = listen_detail_path(&config_path, &repo_root, issue_identifier)?;
    fs::create_dir_all(
        detail_path
            .parent()
            .expect("detail path should have a parent"),
    )?;
    fs::write(
        &detail_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "issue_identifier": issue_identifier,
            "issue_title": "Investigate session detail PR ref",
            "updated_at_epoch_seconds": 1_773_575_180u64,
            "session_updated_at_epoch_seconds": 1_773_575_100u64,
            "phase": "running",
            "summary": "Structured detail keeps PR number only",
            "turns": 2,
            "tokens": {
                "input": 55,
                "output": 13
            },
            "pull_request": {
                "number": 482,
                "status": "draft"
            },
            "references": {
                "branch": "met-27-pr-ref"
            },
            "milestones": [],
            "log_excerpts": []
        }))?,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Detail status: available"))
        .stdout(predicate::str::contains("Detail PR: draft #482"))
        .stdout(predicate::str::contains("Detail PR Ref: #482"))
        .stdout(predicate::str::contains("Detail PR URL").not());

    Ok(())
}

#[test]
fn listen_sessions_inspect_renders_stale_worker_recovery_metadata() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let issue_identifier = "ENG-10744";
    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": format!("{issue_identifier}-id"),
            "issue_identifier": issue_identifier,
            "issue_title": "Recover stale listen workers",
            "project_name": "MetaStack CLI",
            "team_key": "ENG",
            "issue_url": "https://linear.app/metastack-labs/issue/ENG-10744",
            "phase": "running",
            "summary": "Recovered stale worker | pid 51515 | recovery attempts 1/2",
            "brief_path": format!("{}/agents/briefs/{issue_identifier}.md", branding::PROJECT_DIR),
            "backlog_issue_identifier": "TECH-10744",
            "workspace_path": format!("/tmp/{issue_identifier}"),
            "workpad_comment_id": format!("comment-{issue_identifier}"),
            "started_at_epoch_seconds": 1_773_575_000u64,
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "session_id": "codex-session-10744",
            "stale_worker_recovery_attempt_count": 1,
            "latest_stale_worker_failure": {
                "pid": 42424,
                "observed_at_epoch_seconds": 1_773_575_050u64,
                "last_persisted_phase": "running",
                "summary": "worker pid 42424 disappeared while the session was running",
                "classification": {
                    "category": "infra",
                    "reason": "worker died",
                    "retryable": true
                }
            },
            "turns": 2,
            "tokens": {
                "input": 55,
                "output": 13
            },
            "pull_request": {
                "number": 482,
                "status": "draft"
            },
            "log_path": format!("logs/{issue_identifier}.log")
        })],
    )?;

    let detail_path = listen_detail_path(&config_path, &repo_root, issue_identifier)?;
    fs::create_dir_all(
        detail_path
            .parent()
            .expect("detail path should have a parent"),
    )?;
    fs::write(
        &detail_path,
        serde_json::to_vec_pretty(&json!({
            "version": 5,
            "issue_identifier": issue_identifier,
            "issue_title": "Recover stale listen workers",
            "started_at_epoch_seconds": 1_773_575_000u64,
            "updated_at_epoch_seconds": 1_773_575_180u64,
            "session_updated_at_epoch_seconds": 1_773_575_100u64,
            "phase": "running",
            "summary": "Recovered stale worker | pid 51515 | recovery attempts 1/2",
            "stale_worker_recovery_attempt_count": 1,
            "latest_stale_worker_failure": {
                "pid": 42424,
                "observed_at_epoch_seconds": 1_773_575_050u64,
                "last_persisted_phase": "running",
                "summary": "worker pid 42424 disappeared while the session was running",
                "classification": {
                    "category": "infra",
                    "reason": "worker died",
                    "retryable": true
                }
            },
            "turns": 2,
            "tokens": {
                "input": 55,
                "output": 13
            },
            "pull_request": {
                "number": 482,
                "status": "draft"
            },
            "references": {
                "branch": "eng-10744-stale-recovery"
            },
            "milestones": [],
            "log_excerpts": []
        }))?,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Recovery attempts: 1/2"))
        .stdout(predicate::str::contains(
            "Latest stale worker failure: pid 42424",
        ))
        .stdout(predicate::str::contains("Elapsed since start:"))
        .stdout(predicate::str::contains("Detail recovery attempts: 1/2"))
        .stdout(predicate::str::contains(
            "Detail latest stale worker failure: pid 42424",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_inspect_shows_execute_origin_label() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let mut session = listen_session_json("MET-45", "running", 1_773_575_100, Some(99999));
    session["origin"] = json!("execute");

    write_listen_store_session(&config_path, &repo_root, vec![session])?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Origin: Execute"))
        .stdout(predicate::str::contains("MET-45"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_inspect_shows_listen_origin_by_default() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![listen_session_json(
            "MET-50",
            "running",
            1_773_575_100,
            Some(99999),
        )],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Origin: Listen"))
        .stdout(predicate::str::contains("MET-50"));

    Ok(())
}

#[test]
fn listen_render_once_demo_outputs_dashboard_snapshot() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--render-once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Listen Status"))
        .stdout(predicate::str::contains("Watching: all assignees"))
        .stdout(predicate::str::contains("Runtime"))
        .stdout(predicate::str::contains("Agent Sessions"))
        .stdout(predicate::str::contains("http://").not())
        .stdout(predicate::str::contains("127.0.0.1").not())
        .stdout(predicate::str::contains("localhost").not())
        .stdout(predicate::str::contains("SESSION"))
        .stdout(predicate::str::contains("PROGRESS"))
        .stdout(predicate::str::contains("draft #321"))
        .stdout(predicate::str::contains("MET-13"))
        .stdout(predicate::str::contains("MET-17"));

    Ok(())
}

#[test]
fn listen_render_once_demo_can_snapshot_selected_session_detail() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--demo",
            "--render-once",
            "--events",
            "enter",
            "--width",
            "200",
            "--height",
            "56",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Selected Session"))
        .stdout(predicate::str::contains("PR: draft #321"))
        .stdout(predicate::str::contains("Verification JSON:"))
        .stdout(predicate::str::contains("Workpad: comment-met-13"))
        .stdout(predicate::str::contains("Origin: Listen"));

    Ok(())
}

#[test]
fn listen_render_once_demo_detail_shows_execute_origin_for_execute_session()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--demo",
            "--render-once",
            "--events",
            "down,enter",
            "--width",
            "200",
            "--height",
            "58",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Selected Session"))
        .stdout(predicate::str::contains("MET-17"))
        .stdout(predicate::str::contains("Origin: Execute"))
        .stdout(predicate::str::contains(format!(
            "This session was started by `{} agents execute`",
            branding::COMMAND_NAME
        )));

    Ok(())
}

#[test]
fn listen_render_once_demo_events_accept_esc_to_close_selected_session_detail()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--demo",
            "--render-once",
            "--events",
            "enter,esc",
            "--width",
            "200",
            "--height",
            "44",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Sessions"))
        .stdout(predicate::str::contains("Selected Session").not())
        .stdout(predicate::str::contains("MET-13"));

    Ok(())
}

#[test]
fn agents_listen_help_omits_browser_dashboard_flags() {
    let _guard = listen_test_lock();
    meta()
        .args(["agents", "listen", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--all-assignees"))
        .stdout(predicate::str::contains(
            "Agent Sessions and In Progress Issues",
        ))
        .stdout(predicate::str::contains(
            "Press Enter on a selected item to open its detail pane",
        ))
        .stdout(predicate::str::contains(
            "Press P to pause a running session",
        ))
        .stdout(predicate::str::contains("--hide-active-issues"))
        .stdout(predicate::str::contains("--hide-preview"))
        .stdout(predicate::str::contains("--dashboard-port").not())
        .stdout(predicate::str::contains("http://").not());
}

#[test]
fn legacy_listen_help_omits_browser_dashboard_flags() {
    let _guard = listen_test_lock();
    meta()
        .args(["listen", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Interactive dashboard:"))
        .stdout(predicate::str::contains(
            "Press Enter on a selected item to open its detail pane",
        ))
        .stdout(predicate::str::contains(format!(
            "{} listen sessions list",
            branding::COMMAND_NAME
        )))
        .stdout(predicate::str::contains(format!(
            "{} listen sessions inspect --root . --project \"MetaStack API\"",
            branding::COMMAND_NAME
        )))
        .stdout(predicate::str::contains(format!(
            "{} listen sessions clear --root . --project \"MetaStack API\"",
            branding::COMMAND_NAME
        )))
        .stdout(predicate::str::contains(format!(
            "{} agents listen --team MET --project \"MetaStack API\"",
            branding::COMMAND_NAME
        )))
        .stdout(predicate::str::contains("--all-assignees"))
        .stdout(predicate::str::contains("--dashboard-port").not())
        .stdout(predicate::str::contains("http://").not());
}

#[cfg(unix)]
#[test]
fn listen_check_reports_codex_config_status_and_linear_api_validation() -> Result<(), Box<dyn Error>>
{
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let home_dir = temp.path().join("home");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(home_dir.join(".codex"))?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  },
  "agent": {
    "provider": "codex",
    "model": "gpt-5.4",
    "reasoning": "high"
  },
  "validation": {
    "commands": ["cargo test --test listen -- --test-threads=1"],
    "profile": "ticket-proof",
    "repair_attempts": 2
  },
  "listen": {
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    fs::write(
        home_dir.join(".codex/config.toml"),
        r#"approval_policy = "never"
sandbox_mode = "danger-full-access"

[mcp_servers.linear]
enabled = true
"#,
    )?;

    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  cat <<'EOF'
-a, --ask-for-approval <APPROVAL_POLICY>
-s, --sandbox <SANDBOX_MODE>
-C, --cd <DIR>
    --add-dir <DIR>
    --dangerously-bypass-approvals-and-sandbox
EOF
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-m, --model <MODEL>
-c, --config <key=value>
    --json
EOF
  exit 0
fi
exit 0
"#,
    )?;
    let mut permissions = fs::metadata(&codex_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&codex_path, permissions)?;

    init_repo_with_origin(&repo_root)?;
    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("HOME", &home_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "agents",
            "listen",
            "--check",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Listen preflight passed for provider `codex`.",
        ))
        .stdout(predicate::str::contains("approval_policy = \"never\""))
        .stdout(predicate::str::contains(
            "sandbox_mode = \"danger-full-access\"",
        ))
        .stdout(predicate::str::contains("Linear API endpoint is reachable"))
        .stdout(predicate::str::contains(
            "Linear API authentication succeeded.",
        ))
        .stdout(predicate::str::contains(
            "Effective assignee filter: Kames + unassigned",
        ))
        .stdout(predicate::str::contains(
            "Validation profile source: repo_config",
        ))
        .stdout(predicate::str::contains(
            "Validation profile label: ticket-proof",
        ))
        .stdout(predicate::str::contains(
            "Validation commands: cargo test --test listen -- --test-threads=1",
        ))
        .stdout(predicate::str::contains(
            "Verification code review: enabled",
        ))
        .stdout(predicate::str::contains("Verification E2E: enabled"))
        .stdout(predicate::str::contains(
            "Verification battle test count: 0",
        ))
        .stdout(predicate::str::contains(
            "Verification Resolved route key: agents.listen.verification",
        ))
        .stdout(predicate::str::contains(
            "Verification Resolved provider: codex",
        ))
        .stdout(predicate::str::contains(
            "Verification Provider source: repo_default",
        ));
    assert!(viewer_mock.calls() >= 1);

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_check_reports_viewer_only_scope_in_preflight_summary() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let home_dir = temp.path().join("home");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(home_dir.join(".codex"))?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  },
  "agent": {
    "provider": "codex",
    "model": "gpt-5.4",
    "reasoning": "high"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "assignment_scope": "viewer_only"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    fs::write(
        home_dir.join(".codex/config.toml"),
        r#"approval_policy = "never"
sandbox_mode = "danger-full-access"

[mcp_servers.linear]
enabled = true
"#,
    )?;

    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  cat <<'EOF'
-a, --ask-for-approval <APPROVAL_POLICY>
-s, --sandbox <SANDBOX_MODE>
-C, --cd <DIR>
    --add-dir <DIR>
    --dangerously-bypass-approvals-and-sandbox
EOF
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-m, --model <MODEL>
-c, --config <key=value>
    --json
EOF
  exit 0
fi
exit 0
"#,
    )?;
    let mut permissions = fs::metadata(&codex_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&codex_path, permissions)?;

    init_repo_with_origin(&repo_root)?;
    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("HOME", &home_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "agents",
            "listen",
            "--check",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Effective assignee filter: only Kames",
        ));
    assert!(viewer_mock.calls() >= 1);

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_check_reports_workspace_pressure_warning_and_critical_states()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let home_dir = temp.path().join("home");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  },
  "agent": {
    "provider": "codex",
    "model": "gpt-5.4",
    "reasoning": "high"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    write_codex_global_config(&home_dir)?;
    write_codex_help_stub(&bin_dir.join("codex"))?;
    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("HOME", &home_dir)
        .env("METASTACK_TEST_MODE", "1")
        .env("METASTACK_TEST_WORKSPACE_PRESSURE_FIXTURE", "warning-disk")
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "agents",
            "listen",
            "--check",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Workspace pressure: warning."))
        .stdout(predicate::str::contains("Managed workspace footprint:"))
        .stdout(predicate::str::contains(
            "Cleanup guidance: warning pressure detected",
        ));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("HOME", &home_dir)
        .env("METASTACK_TEST_MODE", "1")
        .env(
            "METASTACK_TEST_WORKSPACE_PRESSURE_FIXTURE",
            "critical-memory",
        )
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "agents",
            "listen",
            "--check",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Workspace pressure: critical."))
        .stdout(predicate::str::contains("Memory: critical"))
        .stdout(predicate::str::contains(
            "critical host pressure blocks unattended listen",
        ));

    assert!(viewer_mock.calls() >= 2);

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_inspect_renders_validating_phase() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    write_listen_store_session(
        &config_path,
        &repo_root,
        vec![listen_session_json("ENG-10163", "validating", 300, None)],
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ENG-10163 [Validating]"))
        .stdout(predicate::str::contains("ENG-10163 summary"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_fails_fast_on_codex_preflight_before_linear_auth() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let home_dir = temp.path().join("home");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&home_dir)?;
    write_onboarded_config(&config_path, "")?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  },
  "agent": {
    "provider": "codex",
    "model": "gpt-5.4"
  }
}
"#,
    )?;

    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  cat <<'EOF'
-a, --ask-for-approval <APPROVAL_POLICY>
-s, --sandbox <SANDBOX_MODE>
-C, --cd <DIR>
    --add-dir <DIR>
    --dangerously-bypass-approvals-and-sandbox
EOF
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-m, --model <MODEL>
-c, --config <key=value>
    --json
EOF
  exit 0
fi
exit 0
"#,
    )?;
    let mut permissions = fs::metadata(&codex_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&codex_path, permissions)?;

    init_repo_with_origin(&repo_root)?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env_remove("LINEAR_API_KEY")
        .env("METASTACK_CONFIG", &config_path)
        .env("HOME", &home_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("approval_policy = \"never\""))
        .stderr(predicate::str::contains(
            "sandbox_mode = \"danger-full-access\"",
        ))
        .stderr(predicate::str::contains("LINEAR_API_KEY").not());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_startup_blocks_on_critical_workspace_pressure_before_claim_or_worker_launch()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{payload}}"]
transport = "arg"
"#,
        ),
    )?;
    fs::write(
        bin_dir.join("agent-stub"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TEST_OUTPUT_DIR/agent.log\"\n",
    )?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [],
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("METASTACK_TEST_MODE", "1")
        .env(
            "METASTACK_TEST_WORKSPACE_PRESSURE_FIXTURE",
            "critical-memory",
        )
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "agents",
            "listen",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Critical workspace pressure blocks unattended",
        ))
        .stderr(predicate::str::contains(
            "before any claim or worker launch",
        ))
        .stderr(predicate::str::contains("Workspace pressure: critical."));

    assert_eq!(viewer_mock.calls(), 0);
    assert_eq!(issues_mock.calls(), 0);
    assert!(
        !stub_dir.join("agent.log").exists(),
        "critical pressure should block startup before worker launch"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_render_once_surfaces_workspace_pressure_summary() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    init_repo_with_origin(&repo_root)?;

    let _viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let _issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-32",
                        "identifier": "MET-32",
                        "title": "Later cleanup",
                        "description": "Reconcile completed listener workspace after merge",
                        "url": "https://linear.app/issues/MET-32",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-done",
                            "name": "Done",
                            "type": "completed"
                        }
                    }],
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-32\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-32",
                    "MET-32",
                    "Later cleanup",
                    "Reconcile completed listener workspace after merge",
                    "state-done",
                    "Done",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("METASTACK_TEST_MODE", "1")
        .env("METASTACK_TEST_WORKSPACE_PRESSURE_FIXTURE", "warning-disk")
        .args([
            "listen",
            "--render-once",
            "--width",
            "160",
            "--height",
            "48",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Workspace pressure: warning."))
        .stdout(predicate::str::contains("Managed workspace footprint:"));

    Ok(())
}

#[test]
fn agents_listen_matches_legacy_listen_output() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;

    let legacy = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--render-once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .output()?;
    assert!(legacy.status.success());

    let preferred = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--demo",
            "--render-once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .output()?;
    assert!(preferred.status.success());

    assert_eq!(
        String::from_utf8(legacy.stdout)?,
        String::from_utf8(preferred.stdout)?
    );
    Ok(())
}

#[test]
fn agents_listen_matches_legacy_once_output() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;

    let legacy = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .output()?;
    assert!(legacy.status.success());

    let preferred = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .output()?;
    assert!(preferred.status.success());

    assert_eq!(
        String::from_utf8(legacy.stdout)?,
        String::from_utf8(preferred.stdout)?
    );
    Ok(())
}

#[test]
fn listen_once_json_outputs_machine_readable_poll_result() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;

    let assert = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--demo",
            "--once",
            "--json",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success();

    let payload: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout)?;
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["command"], "agents.listen");
    assert!(payload["result"]["title"].as_str().is_some());
    assert!(payload["result"]["scope"].as_str().is_some());
    assert!(payload["result"]["watch_scope"].as_str().is_some());
    assert!(payload["result"]["cycle_summary"].as_str().is_some());
    assert!(payload["result"]["sessions"].is_array());
    assert!(payload["result"]["notes"].is_array());
    assert!(payload["result"]["state_file"].as_str().is_some());
    assert!(
        payload["result"].get("resolved_agent").is_none(),
        "presentation-only resolved agent state must stay out of --json output"
    );
    assert!(
        payload["result"]["runtime"]["current_epoch_seconds"]
            .as_u64()
            .is_some()
    );

    Ok(())
}

#[test]
fn listen_json_without_once_emits_structured_json_error() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let config_path = temp.path().join("metastack.toml");
    write_onboarded_config(&config_path, "")?;

    let assert = meta()
        .current_dir(temp.path())
        .env("METASTACK_CONFIG", &config_path)
        .args(["agents", "listen", "--json"])
        .assert()
        .failure();

    let payload: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout)?;
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["command"], "agents.listen");
    assert_eq!(payload["error"]["code"], "invalid_input");
    assert_eq!(
        payload["error"]["message"],
        format!(
            "`{} agents listen --json` requires `--once`",
            branding::COMMAND_NAME
        )
    );
    assert!(assert.get_output().stderr.is_empty());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_degraded_429_preserves_existing_session_visibility() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    prepare_listen_repo_with_existing_session(&repo_root, &config_path, &api_url)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(429).body("rate limited");
    });

    let assert = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--once",
            "--json",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success();

    let payload: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout)?;
    assert_eq!(payload["result"]["degraded"]["kind"], json!("transient"));
    assert_eq!(payload["result"]["degraded"]["status_code"], json!(429));
    assert_eq!(
        payload["result"]["sessions"][0]["issue_identifier"],
        json!("ENG-10181")
    );
    assert!(
        payload["result"]["runtime"]["linear_status"]
            .as_str()
            .unwrap_or_default()
            .contains("degraded | transient")
    );

    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(listen_state_path(&config_path, &repo_root)?)?)?;
    assert_eq!(state["degraded"]["kind"], json!("transient"));
    assert_eq!(state["degraded"]["status_code"], json!(429));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ENG-10181"))
        .stdout(predicate::str::contains("Degraded Linear state: transient"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_degraded_503_preserves_existing_session_visibility() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    prepare_listen_repo_with_existing_session(&repo_root, &config_path, &api_url)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(503).body("service unavailable");
    });

    let assert = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--once",
            "--json",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success();

    let payload: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout)?;
    assert_eq!(payload["result"]["degraded"]["kind"], json!("transient"));
    assert_eq!(payload["result"]["degraded"]["status_code"], json!(503));
    assert_eq!(
        payload["result"]["sessions"][0]["issue_identifier"],
        json!("ENG-10181")
    );

    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(listen_state_path(&config_path, &repo_root)?)?)?;
    assert_eq!(state["degraded"]["kind"], json!("transient"));
    assert_eq!(state["degraded"]["status_code"], json!(503));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_degraded_network_failure_preserves_existing_session_visibility()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let api_url = closed_graphql_url()?;
    prepare_listen_repo_with_existing_session(&repo_root, &config_path, &api_url)?;

    let assert = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "listen",
            "--once",
            "--json",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success();

    let payload: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout)?;
    assert_eq!(payload["result"]["degraded"]["kind"], json!("transient"));
    assert!(
        payload["result"]["degraded"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("failed to reach the Linear GraphQL endpoint")
    );
    assert_eq!(
        payload["result"]["sessions"][0]["issue_identifier"],
        json!("ENG-10181")
    );

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ENG-10181"))
        .stdout(predicate::str::contains("Degraded Linear state: transient"));

    Ok(())
}

#[test]
fn listen_uses_repo_configured_poll_interval_by_default() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  },
  "listen": {
    "poll_interval_seconds": 42
  }
}
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Terminal refresh: 1s"))
        .stdout(predicate::str::contains("Linear refresh: 42s"));

    Ok(())
}

#[test]
fn listen_cli_poll_interval_overrides_repo_default() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  },
  "listen": {
    "poll_interval_seconds": 42
  }
}
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--poll-interval",
            "9",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Terminal refresh: 1s"))
        .stdout(predicate::str::contains("Linear refresh: 9s"));

    Ok(())
}

#[test]
fn listen_once_uses_repo_selected_profile_and_project_over_conflicting_global_defaults()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let right_server = MockServer::start();
    let wrong_server = MockServer::start();
    let right_api_url = right_server.url("/graphql");
    let wrong_api_url = wrong_server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "global-token"
api_url = "{wrong_api_url}"
team = "PER"
default_profile = "personal"

[linear.profiles.work]
api_key = "repo-token"
api_url = "{right_api_url}"
team = "MET"

[linear.profiles.personal]
api_key = "personal-token"
api_url = "{wrong_api_url}"
team = "PER"
"#
        ),
    )?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "profile": "work",
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent"
  }
}
"#,
    )?;

    let issues_mock = right_server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "repo-token")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-selected",
                        "identifier": "MET-401",
                        "title": "Repo default listen issue",
                        "description": "Should be observed for this repo",
                        "url": "https://linear.app/issues/401",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": null,
                        "labels": {
                            "nodes": []
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "Repo Project"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }, {
                        "id": "issue-wrong-project",
                        "identifier": "MET-402",
                        "title": "Wrong project issue",
                        "description": "Should be filtered out by the repo project default",
                        "url": "https://linear.app/issues/402",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:01:00Z",
                        "assignee": null,
                        "labels": {
                            "nodes": []
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-2",
                            "name": "Wrong Project"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }, {
                        "id": "issue-wrong-team",
                        "identifier": "PER-403",
                        "title": "Wrong team issue",
                        "description": "Should be filtered out by the repo team default",
                        "url": "https://linear.app/issues/403",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:02:00Z",
                        "assignee": null,
                        "labels": {
                            "nodes": []
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-2",
                            "key": "PER",
                            "name": "Personal"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "Repo Project"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env_remove("LINEAR_API_KEY")
        .env_remove("LINEAR_API_URL")
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Observed 1 Todo issue(s) and"))
        .stdout(predicate::str::contains("Dashboard: terminal summary"))
        .stdout(predicate::str::contains(
            "Skipped MET-401: missing required label `agent`.",
        ))
        .stdout(predicate::str::contains("http://").not())
        .stdout(predicate::str::contains("127.0.0.1").not())
        .stdout(predicate::str::contains("localhost").not())
        .stdout(predicate::str::contains("MET-402").not())
        .stdout(predicate::str::contains("PER-403").not());

    assert_eq!(issues_mock.calls(), 2);
    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_rejects_duplicate_active_listener_lock_for_same_project() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let project_dir = listen_project_store_dir(&config_path, &repo_root, Some("MetaStack CLI"))?;
    fs::create_dir_all(&project_dir)?;
    fs::write(
        project_dir.join("active-listener.lock.json"),
        format!(
            r#"{{
  "pid": {},
  "acquired_at_epoch_seconds": 1773575600,
  "source_root": "{}",
  "metastack_root": "{}"
}}"#,
            std::process::id(),
            listen_source_root(&repo_root)?.display(),
            listen_source_root(&repo_root)?
                .join(branding::PROJECT_DIR)
                .canonicalize()?
                .display()
        ),
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--project",
            "MetaStack CLI",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already owns project"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_allows_active_listener_lock_for_different_project() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let alpha_dir = listen_project_store_dir(&config_path, &repo_root, Some("Alpha"))?;
    fs::create_dir_all(&alpha_dir)?;
    fs::write(
        alpha_dir.join("active-listener.lock.json"),
        format!(
            r#"{{
  "pid": {},
  "acquired_at_epoch_seconds": 1773575600,
  "source_root": "{}",
  "metastack_root": "{}"
}}"#,
            std::process::id(),
            listen_source_root(&repo_root)?.display(),
            listen_source_root(&repo_root)?
                .join(branding::PROJECT_DIR)
                .canonicalize()?
                .display()
        ),
    )?;

    let beta_state_path =
        listen_project_store_dir(&config_path, &repo_root, Some("Beta"))?.join("session.json");
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--project",
            "Beta",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "State file: {}",
            beta_state_path.display()
        )))
        .stdout(predicate::str::contains(alpha_dir.display().to_string()).not());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_recovers_stale_active_listener_lock() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let project_dir = listen_project_store_dir(&config_path, &repo_root, None)?;
    fs::create_dir_all(&project_dir)?;
    let lock_path = project_dir.join("active-listener.lock.json");
    fs::write(
        &lock_path,
        format!(
            r#"{{
  "pid": 999999,
  "acquired_at_epoch_seconds": 1773575600,
  "source_root": "{}",
  "metastack_root": "{}"
}}"#,
            listen_source_root(&repo_root)?.display(),
            listen_source_root(&repo_root)?
                .join(branding::PROJECT_DIR)
                .canonicalize()?
                .display()
        ),
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("State file:"))
        .stdout(predicate::str::contains(
            listen_state_path(&config_path, &repo_root)?
                .to_string_lossy()
                .as_ref(),
        ));
    assert!(!lock_path.exists());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_recovers_corrupt_session_json_from_backup() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![listen_session_json(
            "ENG-10163",
            "blocked",
            1_773_575_100,
            None,
        )],
    )?;
    let backup_path = state_path.with_file_name("session.json.bak");
    fs::copy(&state_path, &backup_path)?;
    fs::write(&state_path, "{ invalid session")?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ENG-10163"));

    let recovered: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    assert_eq!(recovered["sessions"][0]["issue_identifier"], "ENG-10163");

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_rejects_duplicate_listener_when_corrupt_primary_lock_recovers_live_backup()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let project_dir = listen_project_store_dir(&config_path, &repo_root, Some("MetaStack CLI"))?;
    fs::create_dir_all(&project_dir)?;
    let lock_path = project_dir.join("active-listener.lock.json");
    let backup_path = project_dir.join("active-listener.lock.json.bak");
    fs::write(&lock_path, "{ invalid lock")?;
    fs::write(
        &backup_path,
        format!(
            r#"{{
  "pid": {},
  "acquired_at_epoch_seconds": 1773575600,
  "source_root": "{}",
  "metastack_root": "{}"
}}"#,
            std::process::id(),
            listen_source_root(&repo_root)?.display(),
            listen_source_root(&repo_root)?
                .join(branding::PROJECT_DIR)
                .canonicalize()?
                .display()
        ),
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--project",
            "MetaStack CLI",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already owns project"));

    let restored: serde_json::Value = serde_json::from_slice(&fs::read(&lock_path)?)?;
    assert_eq!(restored["pid"], serde_json::json!(std::process::id()));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_omitted_project_uses_repo_default_project_identity() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-default"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let default_project_dir =
        listen_project_store_dir(&config_path, &repo_root, Some("project-default"))?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "State file: {}",
            default_project_dir.join("session.json").display()
        )));

    let metadata = fs::read_to_string(default_project_dir.join("project.json"))?;
    assert!(metadata.contains("\"project_selector\": \"project-default\""));
    assert!(metadata.contains("\"project_label\": \"project-default\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_omitted_project_uses_install_default_project_identity() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(
        &config_path,
        r#"[defaults.linear]
project_id = "project-install"
"#,
    )?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    let install_project_dir =
        listen_project_store_dir(&config_path, &repo_root, Some("project-install"))?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "State file: {}",
            install_project_dir.join("session.json").display()
        )));

    let metadata = fs::read_to_string(install_project_dir.join("project.json"))?;
    assert!(metadata.contains("\"project_selector\": \"project-install\""));
    assert!(metadata.contains("\"project_label\": \"project-install\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_render_once_suppresses_pid_probe_output_across_refreshes() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{}"
"#,
            server.url("/graphql"),
        ),
    )?;

    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-40",
                        "identifier": "MET-40",
                        "title": "Keep the running session clean",
                        "description": "Dashboard output should stay clean while the worker is alive",
                        "url": "https://linear.app/issues/40",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }, {
                        "id": "issue-41",
                        "identifier": "MET-41",
                        "title": "Keep the resumed session clean",
                        "description": "No raw process output should enter the dashboard",
                        "url": "https://linear.app/issues/41",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:01:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }]
                }
            }
        }));
    });
    let issue_40_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-40\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-40",
                    "MET-40",
                    "Keep the running session clean",
                    "Dashboard output should stay clean while the worker is alive",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });
    let issue_41_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-41\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-41",
                    "MET-41",
                    "Keep the resumed session clean",
                    "No raw process output should enter the dashboard",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });
    let update_issue_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": null
                }
            }
        }));
    });

    let ps_path = bin_dir.join("ps");
    fs::write(
        &ps_path,
        format!(
            r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/ps-count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '  PID TTY           TIME CMD\n'
printf '4242 ??         0:00.00 {command} listen-worker --ticket MET-noise\n'
printf 'stderr-noise-from-ps\n' >&2
exit 0
"#,
            command = branding::COMMAND_NAME,
        ),
    )?;
    let mut permissions = fs::metadata(&ps_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&ps_path, permissions)?;

    init_repo_with_origin(&repo_root)?;

    let state_path = listen_state_path(&config_path, &repo_root)?;
    fs::create_dir_all(
        state_path
            .parent()
            .expect("listen state path should have a parent"),
    )?;
    fs::write(
        &state_path,
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "sessions": [
                {
                    "issue_id": "issue-40",
                    "issue_identifier": "MET-40",
                    "issue_title": "Keep the running session clean",
                    "project_name": "MetaStack CLI",
                    "team_key": "MET",
                    "issue_url": "https://linear.app/issues/40",
                    "phase": "running",
                    "summary": "Progress text stays clean",
                    "brief_path": null,
                    "backlog_issue_identifier": null,
                    "backlog_issue_title": null,
                    "backlog_path": null,
                    "workspace_path": null,
                    "branch": "met-40-clean-session",
                    "workpad_comment_id": "comment-40",
                    "updated_at_epoch_seconds": 1_773_575_000u64,
                    "pid": 4242,
                    "session_id": "issue-40",
                    "latest_resume_handle": {
                        "provider": "codex",
                        "id": "019cedb4-2293-7651-b0b4-dfac4af6a640-019cedb4-229b-7453-825e-3e3da4e1bf2a"
                    },
                    "turns": 3,
                    "tokens": {},
                    "log_path": format!("{}/agents/sessions/MET-40.log", branding::PROJECT_DIR)
                },
                {
                    "issue_id": "issue-41",
                    "issue_identifier": "MET-41",
                    "issue_title": "Keep the resumed session clean",
                    "project_name": "MetaStack CLI",
                    "team_key": "MET",
                    "issue_url": "https://linear.app/issues/41",
                    "phase": "running",
                    "summary": "Second progress text stays clean",
                    "brief_path": null,
                    "backlog_issue_identifier": null,
                    "backlog_issue_title": null,
                    "backlog_path": null,
                    "workspace_path": null,
                    "branch": "met-41-clean-session",
                    "workpad_comment_id": "comment-41",
                    "updated_at_epoch_seconds": 1_773_574_900u64,
                    "pid": 4343,
                    "session_id": "issue-41",
                    "latest_resume_handle": {
                        "provider": "claude",
                        "id": "019ceda5-0a41-7ef1-bf96-4f26683c1570-019ceda5-0a57-7820-b050-c05e112d66dd"
                    },
                    "turns": 4,
                    "tokens": {},
                    "log_path": format!("{}/agents/sessions/MET-41.log", branding::PROJECT_DIR)
                }
            ]
        }))?,
    )?;

    let current_path = std::env::var("PATH")?;
    let run_render_once = || -> Result<(String, String), Box<dyn Error>> {
        let output = meta()
            .current_dir(&repo_root)
            .env("METASTACK_CONFIG", &config_path)
            .env("TEST_OUTPUT_DIR", &stub_dir)
            .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
            .args([
                "listen",
                "--root",
                repo_root.to_str().expect("temp path should be utf-8"),
                "--render-once",
                "--width",
                "140",
                "--height",
                "36",
            ])
            .assert()
            .success()
            .get_output()
            .clone();
        Ok((
            String::from_utf8(output.stdout)?,
            String::from_utf8(output.stderr)?,
        ))
    };

    let (first_stdout, first_stderr) = run_render_once()?;
    let (second_stdout, second_stderr) = run_render_once()?;

    for rendered in [&first_stdout, &second_stdout] {
        assert!(rendered.contains("Agent Sessions"));
        assert!(rendered.contains("MET-40"));
        assert!(rendered.contains("MET-41"));
        assert!(rendered.contains("Running"));
        assert!(rendered.contains("n/a"));
        assert!(rendered.contains("019c...e1bf2a"));
        assert!(rendered.contains("019c...2d66dd"));
        assert!(rendered.contains("Progress text stays clean"));
        assert!(rendered.contains("Second progress text stays clean"));
        assert!(!rendered.contains("http://"));
        assert!(!rendered.contains("127.0.0.1"));
        assert!(!rendered.contains("localhost"));
        assert!(!rendered.contains("PID TTY"));
        assert!(!rendered.contains(&format!(
            "{} listen-worker --ticket MET-noise",
            branding::COMMAND_NAME
        )));
    }
    for rendered in [&first_stderr, &second_stderr] {
        assert!(!rendered.contains("stderr-noise-from-ps"));
    }

    assert_eq!(
        fs::read_to_string(stub_dir.join("ps-count.txt"))?.trim(),
        "4"
    );

    let state = fs::read_to_string(&state_path)?;
    assert!(state.contains("\"issue_identifier\": \"MET-40\""));
    assert!(state.contains("\"issue_identifier\": \"MET-41\""));
    assert_eq!(state.matches("\"phase\": \"running\"").count(), 2);

    assert!(issues_mock.calls() >= 2);
    assert!(issue_40_mock.calls() >= 2);
    assert!(issue_41_mock.calls() >= 2);
    update_issue_mock.assert_calls(0);

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_uses_the_same_project_identity_for_repo_and_worktree_roots() -> Result<(), Box<dyn Error>>
{
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;
    let worktree_root = create_worktree_checkout(&repo_root, "feature/listen", "repo-worktree")?;

    let repo_store_dir = listen_project_store_dir(&config_path, &repo_root, None)?;
    let worktree_store_dir = listen_project_store_dir(&config_path, &worktree_root, None)?;
    assert_eq!(repo_store_dir, worktree_store_dir);

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success();

    meta()
        .current_dir(&worktree_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            worktree_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Source root: {}",
            repo_root.canonicalize()?.display()
        )))
        .stdout(predicate::str::contains(format!(
            "Lock file: {}",
            repo_store_dir.join("active-listener.lock.json").display()
        )));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_bootstraps_workspace_clone_workpad_and_agent_session() -> Result<(), Box<dyn Error>>
{
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer",
    "instructions_path": "instructions/listen.md"
  }
}
"#,
    )?;
    fs::create_dir_all(repo_root.join("instructions"))?;
    fs::write(
        repo_root.join("instructions/listen.md"),
        "# Listener Instructions\nKeep the workpad current.\n",
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;
    let stub_path = bin_dir.join("agent-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$PWD" > "$TEST_OUTPUT_DIR/cwd.txt"
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload.txt"
printf '%s' "$METASTACK_LINEAR_WORKPAD_COMMENT_ID" > "$TEST_OUTPUT_DIR/workpad.txt"
printf '%s' "$METASTACK_AGENT_INSTRUCTIONS" > "$TEST_OUTPUT_DIR/instructions.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-21",
                        "identifier": "MET-21",
                        "title": "Daemon pickup flow",
                        "description": "Claim Todo work and create agent briefs",
                        "url": "https://linear.app/issues/21",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }, {
                        "id": "issue-36",
                        "identifier": "MET-36",
                        "title": "Technical: Daemon pickup flow",
                        "description": "# Technical: Daemon pickup flow\n",
                        "url": "https://linear.app/issues/36",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:01:00Z",
                        "assignee": null,
                        "labels": {
                            "nodes": []
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-backlog",
                            "name": "Backlog",
                            "type": "backlog"
                        }
                    }, {
                        "id": "issue-22",
                        "identifier": "MET-22",
                        "title": "Other project work",
                        "description": "Should not be claimed by this repo default",
                        "url": "https://linear.app/issues/22",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:02:00Z",
                        "assignee": {
                            "id": "viewer-2",
                            "name": "Someone Else",
                            "email": "else@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-2",
                            "name": "Another Project"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });

    let teams_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(json!({
            "data": {
                "teams": {
                    "nodes": [{
                        "id": "team-1",
                        "key": "MET",
                        "name": "Metastack",
                        "states": {
                            "nodes": [
                                {
                                    "id": "state-backlog",
                                    "name": "Backlog",
                                    "type": "backlog"
                                },
                                {
                                    "id": "state-1",
                                    "name": "Todo",
                                    "type": "unstarted"
                                },
                                {
                                    "id": "state-2",
                                    "name": "In Progress",
                                    "type": "started"
                                }
                            ]
                        }
                    }]
                }
            }
        }));
    });
    let _projects_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Projects");
        then.status(200).json_body(json!({
            "data": {
                "projects": {
                    "nodes": [{
                        "id": "project-1",
                        "name": "MetaStack CLI",
                        "description": null,
                        "url": "https://linear.app/projects/project-1",
                        "progress": 0.5,
                        "teams": {
                            "nodes": [{
                                "id": "team-1",
                                "key": "MET",
                                "name": "Metastack"
                            }]
                        }
                    }]
                }
            }
        }));
    });

    let issue_detail_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-21\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-21",
                    "MET-21",
                    "Daemon pickup flow",
                    "Claim Todo work and create agent briefs",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-21",
                        "identifier": "MET-21",
                        "title": "Daemon pickup flow",
                        "description": "Claim Todo work and create agent briefs",
                        "url": "https://linear.app/issues/21",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });

    let create_backlog_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue");
        then.status(500);
    });

    let comment_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-21",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateComment");
        then.status(200).json_body(json!({
            "data": {
                "commentUpdate": {
                    "success": true,
                    "comment": {
                        "id": "comment-21",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    let state_path = listen_state_path(&config_path, &repo_root)?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 claimed this cycle"))
        .stdout(predicate::str::contains("MET-21"))
        .stdout(predicate::str::contains(
            state_path.to_string_lossy().as_ref(),
        ));

    let workspace_root = temp.path().join("repo-workspace/MET-21");
    assert!(workspace_root.is_dir());
    assert_eq!(
        git_stdout(
            &workspace_root,
            &["rev-parse", "--path-format=absolute", "--git-dir"]
        )?,
        git_stdout(
            &workspace_root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"]
        )?
    );
    assert!(
        !git_stdout(&repo_root, &["worktree", "list"])?
            .contains(workspace_root.to_string_lossy().as_ref())
    );

    let brief = fs::read_to_string(
        workspace_root.join(format!("{}/agents/briefs/MET-21.md", branding::PROJECT_DIR)),
    )?;
    assert!(brief.contains("Daemon pickup flow"));
    assert!(brief.contains(&format!(
        "Picked up automatically by `{} listen`.",
        branding::COMMAND_NAME
    )));

    wait_for_path(&stub_dir.join("payload.txt"))?;
    wait_for_path(&stub_dir.join("workpad.txt"))?;
    wait_for_path(&stub_dir.join("instructions.txt"))?;
    assert_eq!(
        fs::read_to_string(stub_dir.join("workpad.txt"))?,
        "comment-21"
    );
    let instructions = fs::read_to_string(stub_dir.join("instructions.txt"))?;
    assert!(instructions.contains("## Built-in Workflow Contract"));
    assert!(instructions.contains("No repo overlay files were found"));
    assert!(instructions.contains("Shared automation keeps the"));
    assert!(instructions.contains("metastack"));
    assert!(instructions.contains("label attached"));
    assert!(instructions.contains("do not use the legacy `symphony` label"));
    let backlog_index_path =
        workspace_root.join(format!("{}/backlog/MET-21/index.md", branding::PROJECT_DIR));
    assert!(
        backlog_index_path.is_file(),
        "expected backlog index at {}\nstate: {:?}\nbacklog root: {}\nworkspace entries: {:?}",
        backlog_index_path.display(),
        listen_state_path(&config_path, &repo_root)
            .ok()
            .and_then(|path| fs::read_to_string(path).ok()),
        workspace_root
            .join(branding::PROJECT_DIR)
            .join("backlog")
            .display(),
        fs::read_dir(&workspace_root)
            .map(|entries| entries.count())
            .ok()
    );
    let backlog_index = fs::read_to_string(&backlog_index_path)?;
    assert!(backlog_index.contains("## Requirements"));
    assert!(backlog_index.contains("Claim Todo work and create agent briefs"));
    let validation_plan = fs::read_to_string(workspace_root.join(format!(
        "{}/backlog/MET-21/validation.md",
        branding::PROJECT_DIR
    )))?;
    assert!(validation_plan.contains("must not overwrite the primary Linear issue description"));
    assert!(validation_plan.contains("Update the existing `## Codex Workpad` comment"));
    assert!(validation_plan.contains("artifacts/validation/MET-21.md"));
    assert!(
        validation_plan
            .contains("Do not write ticket-specific PR evidence to repo-root `validation.md`")
    );
    assert!(!validation_plan.contains(&format!("{} sync push MET-21", branding::COMMAND_NAME)));
    assert!(
        workspace_root
            .join(format!(
                "{}/backlog/MET-21/.linear.json",
                branding::PROJECT_DIR
            ))
            .is_file()
    );

    assert!(viewer_mock.calls() >= 1);
    teams_mock.assert_calls(1);
    assert!(issue_detail_mock.calls() >= 1);
    create_backlog_mock.assert_calls(0);
    assert!(comment_mock.calls() >= 1);

    assert!(
        state_path.is_file(),
        "expected listen state at {}",
        state_path.display()
    );
    let state = fs::read_to_string(&state_path)?;
    assert!(state.contains("\"issue_identifier\": \"MET-21\""));
    assert!(
        state.contains("\"phase\": \"running\"")
            || state.contains("\"phase\": \"reviewing\"")
            || state.contains("\"phase\": \"final-review\"")
            || state.contains("\"phase\": \"validating\"")
            || state.contains("\"phase\": \"publishing\"")
            || state.contains("\"phase\": \"blocked\"")
            || state.contains("\"phase\": \"completed\""),
        "expected an active or finished worker phase in state: {state}"
    );
    assert!(state.contains("\"workpad_comment_id\": \"comment-21\""));
    assert!(state.contains("\"workspace_path\":"));
    assert!(state.contains("\"backlog_issue_identifier\": \"MET-21\""));
    assert!(!state.contains("\"backlog_issue_identifier\": \"MET-36\""));
    assert!(!state.contains("\"issue_identifier\": \"MET-22\""));
    assert!(
        !repo_root
            .join(format!(
                "{}/agents/sessions/listen-state.json",
                branding::PROJECT_DIR
            ))
            .exists()
    );

    let inspect = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inspect = String::from_utf8_lossy(&inspect);
    assert!(inspect.contains(state_path.to_string_lossy().as_ref()));
    assert!(inspect.contains("Tracked sessions:"));
    assert!(inspect.contains("MET-21"));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Stored MetaListen project sessions",
        ))
        .stdout(predicate::str::contains("repo"));

    let project_key = state_path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .expect("project key should be present")
        .to_string();
    meta()
        .current_dir(temp.path())
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "resume",
            "--project-key",
            &project_key,
            "--demo",
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            state_path.to_string_lossy().as_ref(),
        ));

    let latest_state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    if let Some(pid) = latest_state["sessions"]
        .as_array()
        .and_then(|sessions| sessions.first())
        .and_then(|session| session.get("pid"))
        .and_then(serde_json::Value::as_u64)
    {
        let _ = ProcessCommand::new("kill").arg(pid.to_string()).status();
    }

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_sessions_target_multiple_project_scopes_from_one_repo() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    fs::create_dir_all(&repo_root)?;
    write_onboarded_config(&config_path, "")?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-default"
  }
}
"#,
    )?;
    init_repo_with_origin(&repo_root)?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success();
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--demo",
            "--once",
            "--project",
            "project-beta",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success();

    let default_state_path =
        listen_project_store_dir(&config_path, &repo_root, Some("project-default"))?
            .join("session.json");
    let beta_state_path = listen_project_store_dir(&config_path, &repo_root, Some("project-beta"))?
        .join("session.json");

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("project-default"))
        .stdout(predicate::str::contains("project-beta"))
        .stdout(predicate::str::contains(
            repo_root.canonicalize()?.display().to_string(),
        ));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            default_state_path.display().to_string(),
        ))
        .stdout(predicate::str::contains("Project: project-default"));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--project",
            "project-beta",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            beta_state_path.display().to_string(),
        ))
        .stdout(predicate::str::contains("Project: project-beta"));

    let beta_project_key = beta_state_path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .expect("beta project key should be present")
        .to_string();

    meta()
        .current_dir(temp.path())
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--project-key",
            &beta_project_key,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            beta_state_path.display().to_string(),
        ))
        .stdout(predicate::str::contains("Project: project-beta"));

    meta()
        .current_dir(temp.path())
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "resume",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--project",
            "project-beta",
            "--demo",
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            beta_state_path.display().to_string(),
        ));

    meta()
        .current_dir(temp.path())
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "clear",
            "--project-key",
            &beta_project_key,
            "--all",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("project-beta"));

    assert!(default_state_path.parent().is_some_and(Path::exists));
    assert!(beta_state_path.parent().is_some_and(Path::exists));
    if beta_state_path.is_file() {
        let beta_state: serde_json::Value = serde_json::from_slice(&fs::read(&beta_state_path)?)?;
        assert_eq!(
            beta_state["sessions"]
                .as_array()
                .expect("sessions should remain an array")
                .len(),
            0
        );
    }

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_prefers_command_route_agent_over_global_default() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "global-stub"

[agents.routing.commands."agents.listen"]
provider = "listen-stub"

[agents.commands.global-stub]
command = "global-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.listen-stub]
command = "listen-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;

    let global_stub_path = bin_dir.join("global-stub");
    fs::write(
        &global_stub_path,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/global.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&global_stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&global_stub_path, permissions)?;

    let listen_stub_path = bin_dir.join("listen-stub");
    fs::write(
        &listen_stub_path,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/listen.txt"
printf '%s' "$METASTACK_AGENT_PROVIDER_SOURCE" > "$TEST_OUTPUT_DIR/provider-source.txt"
printf '%s' "$METASTACK_AGENT_ROUTE_KEY" > "$TEST_OUTPUT_DIR/route-key.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&listen_stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&listen_stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-63",
                        "identifier": "MET-63",
                        "title": "Route listen agent",
                        "description": "Verify listen routing",
                        "url": "https://linear.app/issues/63",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(json!({
            "data": {
                "teams": {
                    "nodes": [{
                        "id": "team-1",
                        "key": "MET",
                        "name": "Metastack",
                        "states": {
                            "nodes": [
                                {
                                    "id": "state-1",
                                    "name": "Todo",
                                    "type": "unstarted"
                                },
                                {
                                    "id": "state-2",
                                    "name": "In Progress",
                                    "type": "started"
                                }
                            ]
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Projects");
        then.status(200).json_body(json!({
            "data": {
                "projects": {
                    "nodes": [{
                        "id": "project-1",
                        "name": "MetaStack CLI",
                        "description": null,
                        "url": "https://linear.app/projects/project-1",
                        "progress": 0.5,
                        "teams": {
                            "nodes": [{
                                "id": "team-1",
                                "key": "MET",
                                "name": "Metastack"
                            }]
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-63\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-63",
                    "MET-63",
                    "Route listen agent",
                    "Verify listen routing",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-63",
                        "identifier": "MET-63",
                        "title": "Route listen agent",
                        "description": "Verify listen routing",
                        "url": "https://linear.app/issues/63",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue");
        then.status(500);
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-63",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-63"));

    wait_for_path(&stub_dir.join("listen.txt"))?;
    wait_for_path(&stub_dir.join("provider-source.txt"))?;
    wait_for_path(&stub_dir.join("route-key.txt"))?;
    assert!(stub_dir.join("listen.txt").exists());
    assert!(!stub_dir.join("global.txt").exists());
    assert_eq!(
        fs::read_to_string(stub_dir.join("provider-source.txt"))?,
        "command_route:agents.listen"
    );
    assert_eq!(
        fs::read_to_string(stub_dir.join("route-key.txt"))?,
        "agents.listen"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_downloads_issue_attachment_context_for_agent() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;
    let stub_path = bin_dir.join("agent-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload.txt"
printf '%s' "$METASTACK_AGENT_INSTRUCTIONS" > "$TEST_OUTPUT_DIR/instructions.txt"
printf '%s' "$METASTACK_LINEAR_ATTACHMENT_CONTEXT_PATH" > "$TEST_OUTPUT_DIR/attachment-context-path.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-24",
                        "identifier": "MET-24",
                        "title": "Attachment bootstrap",
                        "description": "Use uploaded docs as implementation context",
                        "url": "https://linear.app/issues/24",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-24\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-24",
                    "MET-24",
                    "Attachment bootstrap",
                    "Use uploaded docs as implementation context",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    vec![
                        json!({
                            "id": "attachment-1",
                            "title": "specification.md",
                            "url": server.url("/downloads/specification.md"),
                            "sourceType": "upload",
                            "metadata": {}
                        }),
                        json!({
                            "id": "attachment-2",
                            "title": "diagram.png",
                            "url": server.url("/downloads/diagram.png"),
                            "sourceType": "upload",
                            "metadata": {}
                        })
                    ],
                    Vec::new(),
                )
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-24",
                        "identifier": "MET-24",
                        "title": "Attachment bootstrap",
                        "description": "Use uploaded docs as implementation context",
                        "url": "https://linear.app/issues/24",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/downloads/specification.md");
        then.status(200).body("# Downloaded specification\n");
    });

    server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/downloads/diagram.png");
        then.status(200).body("fake-png");
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-24",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-24"));

    wait_for_path(&stub_dir.join("attachment-context-path.txt"))?;
    let workspace_root = temp.path().join("repo-workspace/MET-24");
    let context_dir = workspace_root.join(format!(
        "{}/agents/issue-context/MET-24",
        branding::PROJECT_DIR
    ));
    let reported_context_dir = PathBuf::from(fs::read_to_string(
        stub_dir.join("attachment-context-path.txt"),
    )?);
    assert_eq!(
        reported_context_dir.canonicalize()?,
        context_dir.canonicalize()?
    );

    let manifest = fs::read_to_string(context_dir.join("README.md"))?;
    assert!(manifest.contains("Files downloaded: 2"));
    assert!(manifest.contains("files/01-specification.md"));
    assert!(manifest.contains("files/02-diagram.png"));
    assert_eq!(
        fs::read_to_string(context_dir.join("files/01-specification.md"))?,
        "# Downloaded specification\n"
    );
    assert_eq!(
        fs::read(context_dir.join("files/02-diagram.png"))?,
        b"fake-png"
    );

    let payload = fs::read_to_string(stub_dir.join("payload.txt"))?;
    let instructions = fs::read_to_string(stub_dir.join("instructions.txt"))?;
    assert!(payload.contains("Attachment context:"));
    assert!(payload.contains("Attachment manifest:"));
    assert!(instructions.contains("Additional Linear attachment context has been downloaded"));
    assert!(instructions.contains("## Repository Scope"));
    assert!(instructions.contains("Active workspace checkout:"));
    assert!(instructions.contains(
        "Keep implementation, validation, and local backlog updates anchored to the provided workspace checkout"
    ));
    assert!(!instructions.contains("MetaStack CLI"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_refreshes_existing_workspace_clone_and_reuses_backlog_and_workpad_comment()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer",
    "instructions_path": "instructions/listen.md"
  }
}
"#,
    )?;
    fs::create_dir_all(repo_root.join("instructions"))?;
    fs::write(
        repo_root.join("instructions/listen.md"),
        "# Listener Instructions\nKeep the workpad current.\n",
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;
    let stub_path = bin_dir.join("agent-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace_root = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-50")?;
    let status = ProcessCommand::new("git")
        .args([
            "-C",
            workspace_root.to_string_lossy().as_ref(),
            "checkout",
            "-b",
            "scratch-local",
        ])
        .status()?;
    assert!(status.success());
    fs::write(workspace_root.join("stale.txt"), "stale\n")?;
    for args in [
        vec![
            "-C",
            workspace_root.to_string_lossy().as_ref(),
            "add",
            "stale.txt",
        ],
        vec![
            "-C",
            workspace_root.to_string_lossy().as_ref(),
            "commit",
            "-m",
            "stale workspace commit",
        ],
    ] {
        let status = ProcessCommand::new("git").args(args).status()?;
        assert!(status.success());
    }
    let backlog_dir = workspace_root.join(format!("{}/backlog/MET-50", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# Existing Technical Backlog\n\nDo not overwrite me.\n",
    )?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-50",
                        "identifier": "MET-50",
                        "title": "Reuse existing listener workspace",
                        "description": "Resume the current backlog inside the existing workspace clone",
                        "url": "https://linear.app/issues/50",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }, {
                        "id": "issue-51",
                        "identifier": "MET-51",
                        "title": "Technical: Reuse existing listener workspace",
                        "description": "# Existing Technical Backlog\n\nDo not overwrite me.\n",
                        "url": "https://linear.app/issues/51",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:01:00Z",
                        "assignee": null,
                        "labels": {
                            "nodes": []
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-backlog",
                            "name": "Backlog",
                            "type": "backlog"
                        }
                    }]
                }
            }
        }));
    });
    let teams_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });
    let _projects_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Projects");
        then.status(200).json_body(json!({
            "data": {
                "projects": {
                    "nodes": [{
                        "id": "project-1",
                        "name": "MetaStack CLI",
                        "description": "CLI platform work",
                        "url": "https://linear.app/projects/1",
                        "progress": 0.42,
                        "teams": {
                            "nodes": [{
                                "id": "team-1",
                                "key": "MET",
                                "name": "Metastack"
                            }]
                        }
                    }]
                }
            }
        }));
    });
    let update_issue_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-50",
                        "identifier": "MET-50",
                        "title": "Reuse existing listener workspace",
                        "description": "Resume the current backlog inside the existing workspace clone",
                        "url": "https://linear.app/issues/50",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });
    let parent_detail_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-50\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-50",
                    "MET-50",
                    "Reuse existing listener workspace",
                    "Resume the current backlog inside the existing workspace clone",
                    "state-2",
                    "In Progress",
                    vec![json!({
                        "id": "comment-50",
                        "body": "## Codex Workpad\n",
                        "resolvedAt": null
                    })],
                    Vec::new(),
                    vec![json!({
                        "id": "issue-51",
                        "identifier": "MET-51",
                        "title": "Technical: Reuse existing listener workspace",
                        "url": "https://linear.app/issues/51"
                    })],
                )
            }
        }));
    });
    let _update_comment_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateComment")
            .body_includes("\"id\":\"comment-50\"")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentUpdate": {
                    "success": true,
                    "comment": {
                        "id": "comment-50",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });
    let create_backlog_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue");
        then.status(500);
    });
    let create_comment_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment");
        then.status(500);
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-50"));

    assert!(viewer_mock.calls() >= 1);
    teams_mock.assert_calls(1);
    update_issue_mock.assert_calls(1);
    parent_detail_mock.assert_calls(1);
    create_backlog_mock.assert_calls(0);
    create_comment_mock.assert_calls(0);

    wait_for_path(&stub_dir.join("payload.txt"))?;
    let backlog_content = fs::read_to_string(backlog_dir.join("index.md"))?;
    assert!(backlog_content.starts_with("# Existing Technical Backlog\n\nDo not overwrite me.\n"));
    assert!(!workspace_root.join("stale.txt").exists());
    assert_eq!(
        git_stdout(&workspace_root, &["rev-parse", "--abbrev-ref", "HEAD"])?,
        "met-50-reuse-existing-listener-workspace"
    );
    assert_eq!(
        git_stdout(
            &workspace_root,
            &["rev-parse", "--path-format=absolute", "--git-dir"]
        )?,
        git_stdout(
            &workspace_root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"]
        )?
    );
    assert!(
        fs::read_to_string(stub_dir.join("payload.txt"))?.contains("Backlog identifier: MET-50")
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_executes_repo_selected_builtin_claude_provider() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "agent": {
    "provider": "claude",
    "model": "sonnet",
    "reasoning": "high"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "codex"
default_model = "gpt-5.4"
default_reasoning = "low"
"#,
        ),
    )?;

    let claude_path = bin_dir.join("claude");
    fs::write(
        &claude_path,
        r#"#!/bin/sh
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-p, --print
--model <model>
--effort <level>
--verbose
--output-format <format>
--permission-mode <mode>
EOF
  exit 0
fi
payload=$(cat)
printf '%s\n' "$@" > "$TEST_OUTPUT_DIR/claude-args.txt"
printf '%s' "$payload" > "$TEST_OUTPUT_DIR/prompt.txt"
printf '%s' "$METASTACK_AGENT_NAME" > "$TEST_OUTPUT_DIR/agent.txt"
printf '%s' "$METASTACK_AGENT_MODEL" > "$TEST_OUTPUT_DIR/model.txt"
printf '%s' "$METASTACK_AGENT_REASONING" > "$TEST_OUTPUT_DIR/reasoning.txt"
printf '%s' "$METASTACK_AGENT_PROVIDER_SOURCE" > "$TEST_OUTPUT_DIR/provider-source.txt"
printf '%s' "$METASTACK_AGENT_ROUTE_KEY" > "$TEST_OUTPUT_DIR/route-key.txt"
printf '%s\n' '{"type":"message_start","message":{"usage":{"input_tokens":210}}}'
printf '%s\n' '{"type":"message_delta","usage":{"output_tokens":34}}'
printf '%s' '{"type":"result","subtype":"success","result":"claude listen ok","session_id":"listen-session-1"}'
"#,
    )?;
    let mut permissions = fs::metadata(&claude_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&claude_path, permissions)?;

    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  cat <<'EOF'
-a, --ask-for-approval <APPROVAL_POLICY>
-s, --sandbox <SANDBOX_MODE>
-C, --cd <DIR>
    --add-dir <DIR>
    --dangerously-bypass-approvals-and-sandbox
EOF
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-m, --model <MODEL>
-c, --config <key=value>
    --json
EOF
  exit 0
fi
printf 'codex fallback invoked' > "$TEST_OUTPUT_DIR/codex.txt"
exit 99
"#,
    )?;
    let mut permissions = fs::metadata(&codex_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&codex_path, permissions)?;

    init_repo_with_origin(&repo_root)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-64",
                        "identifier": "MET-64",
                        "title": "Builtin Claude listen agent",
                        "description": "Verify repo-selected builtin provider resolution",
                        "url": "https://linear.app/issues/64",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(json!({
            "data": {
                "teams": {
                    "nodes": [{
                        "id": "team-1",
                        "key": "MET",
                        "name": "Metastack",
                        "states": {
                            "nodes": [
                                {
                                    "id": "state-1",
                                    "name": "Todo",
                                    "type": "unstarted"
                                },
                                {
                                    "id": "state-2",
                                    "name": "In Progress",
                                    "type": "started"
                                }
                            ]
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Projects");
        then.status(200).json_body(json!({
            "data": {
                "projects": {
                    "nodes": [{
                        "id": "project-1",
                        "name": "MetaStack CLI",
                        "description": null,
                        "url": "https://linear.app/projects/project-1",
                        "progress": 0.5,
                        "teams": {
                            "nodes": [{
                                "id": "team-1",
                                "key": "MET",
                                "name": "Metastack"
                            }]
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-64\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-64",
                    "MET-64",
                    "Builtin Claude listen agent",
                    "Verify repo-selected builtin provider resolution",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-64",
                        "identifier": "MET-64",
                        "title": "Builtin Claude listen agent",
                        "description": "Verify repo-selected builtin provider resolution",
                        "url": "https://linear.app/issues/64",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue");
        then.status(500);
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-64",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-64"));

    wait_for_path(&stub_dir.join("claude-args.txt"))?;
    wait_for_path(&stub_dir.join("prompt.txt"))?;
    wait_for_path(&stub_dir.join("provider-source.txt"))?;
    assert!(!stub_dir.join("codex.txt").exists());

    let args = fs::read_to_string(stub_dir.join("claude-args.txt"))?;
    assert!(args.contains("--permission-mode=bypassPermissions"));
    assert!(args.contains("--verbose"));
    assert!(args.contains("--output-format=stream-json"));
    assert!(args.contains("-p"));
    assert!(args.contains("--model=sonnet"));
    assert!(args.contains("--effort=high"));
    assert!(args.contains("--output-format=json"));
    assert!(!args.contains("--reasoning="));
    assert_eq!(fs::read_to_string(stub_dir.join("agent.txt"))?, "claude");
    assert_eq!(fs::read_to_string(stub_dir.join("model.txt"))?, "sonnet");
    assert_eq!(fs::read_to_string(stub_dir.join("reasoning.txt"))?, "high");
    assert_eq!(
        fs::read_to_string(stub_dir.join("provider-source.txt"))?,
        "repo_default"
    );
    assert_eq!(
        fs::read_to_string(stub_dir.join("route-key.txt"))?,
        "agents.listen"
    );
    assert!(
        fs::read_to_string(stub_dir.join("prompt.txt"))?.contains("Builtin Claude listen agent")
    );

    let listen_log = fs::read_to_string(listen_log_path(&config_path, &repo_root, "MET-64")?)?;
    assert!(listen_log.contains("Resolved provider: claude"));
    assert!(listen_log.contains("Resolved model: sonnet"));
    assert!(listen_log.contains("Resolved reasoning: high"));
    assert!(listen_log.contains("Provider source: repo_default"));

    let state_path = listen_state_path(&config_path, &repo_root)?;
    let detail_path = listen_detail_path(&config_path, &repo_root, "MET-64")?;
    wait_for_json_pointer_value(
        &state_path,
        "/sessions/0/canonical/provider",
        &json!("claude"),
    )?;
    wait_for_json_pointer_value(&state_path, "/sessions/0/canonical/model", &json!("sonnet"))?;
    wait_for_json_pointer_value(
        &state_path,
        "/sessions/0/canonical/reasoning",
        &json!("high"),
    )?;
    wait_for_json_pointer_value(&detail_path, "/canonical/provider", &json!("claude"))?;
    wait_for_json_pointer_value(&detail_path, "/canonical/model", &json!("sonnet"))?;
    wait_for_json_pointer_value(&detail_path, "/canonical/reasoning", &json!("high"))?;

    let state: serde_json::Value = serde_json::from_str(&fs::read_to_string(state_path)?)?;
    assert_eq!(
        state.pointer("/sessions/0/canonical/provider"),
        Some(&json!("claude"))
    );
    assert_eq!(
        state.pointer("/sessions/0/canonical/model"),
        Some(&json!("sonnet"))
    );
    assert_eq!(
        state.pointer("/sessions/0/canonical/reasoning"),
        Some(&json!("high"))
    );

    let detail: serde_json::Value = serde_json::from_str(&fs::read_to_string(detail_path)?)?;
    assert_eq!(
        detail.pointer("/canonical/provider"),
        Some(&json!("claude"))
    );
    assert_eq!(detail.pointer("/canonical/model"), Some(&json!("sonnet")));
    assert_eq!(detail.pointer("/canonical/reasoning"), Some(&json!("high")));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_persists_repo_selected_builtin_claude_canonical_metadata_after_turn_failure()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "agent": {
    "provider": "claude",
    "model": "sonnet",
    "reasoning": "high"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "codex"
default_model = "gpt-5.4"
default_reasoning = "low"
"#,
        ),
    )?;

    let claude_path = bin_dir.join("claude");
    fs::write(
        &claude_path,
        r#"#!/bin/sh
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-p, --print
--model <model>
--effort <level>
--verbose
--output-format <format>
--permission-mode <mode>
EOF
  exit 0
fi
payload=$(cat)
printf '%s\n' "$@" > "$TEST_OUTPUT_DIR/claude-args.txt"
printf '%s' "$payload" > "$TEST_OUTPUT_DIR/prompt.txt"
printf '%s' "$METASTACK_AGENT_NAME" > "$TEST_OUTPUT_DIR/agent.txt"
printf '%s' "$METASTACK_AGENT_MODEL" > "$TEST_OUTPUT_DIR/model.txt"
printf '%s' "$METASTACK_AGENT_REASONING" > "$TEST_OUTPUT_DIR/reasoning.txt"
printf '%s' "$METASTACK_AGENT_PROVIDER_SOURCE" > "$TEST_OUTPUT_DIR/provider-source.txt"
printf '%s' "$METASTACK_AGENT_ROUTE_KEY" > "$TEST_OUTPUT_DIR/route-key.txt"
printf '%s\n' '{"type":"message_start","message":{"usage":{"input_tokens":210}}}'
printf '%s\n' '{"type":"message_delta","usage":{"output_tokens":34}}'
printf '%s\n' 'claude listen failed intentionally' >&2
exit 23
"#,
    )?;
    let mut permissions = fs::metadata(&claude_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&claude_path, permissions)?;

    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  cat <<'EOF'
-a, --ask-for-approval <APPROVAL_POLICY>
-s, --sandbox <SANDBOX_MODE>
-C, --cd <DIR>
    --add-dir <DIR>
    --dangerously-bypass-approvals-and-sandbox
EOF
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-m, --model <MODEL>
-c, --config <key=value>
    --json
EOF
  exit 0
fi
printf 'codex fallback invoked' > "$TEST_OUTPUT_DIR/codex.txt"
exit 99
"#,
    )?;
    let mut permissions = fs::metadata(&codex_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&codex_path, permissions)?;

    init_repo_with_origin(&repo_root)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-65",
                        "identifier": "MET-65",
                        "title": "Builtin Claude listen failure path",
                        "description": "Verify failed builtin provider turns still persist canonical metadata",
                        "url": "https://linear.app/issues/65",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(json!({
            "data": {
                "teams": {
                    "nodes": [{
                        "id": "team-1",
                        "key": "MET",
                        "name": "Metastack",
                        "states": {
                            "nodes": [
                                {
                                    "id": "state-1",
                                    "name": "Todo",
                                    "type": "unstarted"
                                },
                                {
                                    "id": "state-2",
                                    "name": "In Progress",
                                    "type": "started"
                                }
                            ]
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Projects");
        then.status(200).json_body(json!({
            "data": {
                "projects": {
                    "nodes": [{
                        "id": "project-1",
                        "name": "MetaStack CLI",
                        "description": null,
                        "url": "https://linear.app/projects/project-1",
                        "progress": 0.5,
                        "teams": {
                            "nodes": [{
                                "id": "team-1",
                                "key": "MET",
                                "name": "Metastack"
                            }]
                        }
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-65\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-65",
                    "MET-65",
                    "Builtin Claude listen failure path",
                    "Verify failed builtin provider turns still persist canonical metadata",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-65",
                        "identifier": "MET-65",
                        "title": "Builtin Claude listen failure path",
                        "description": "Verify failed builtin provider turns still persist canonical metadata",
                        "url": "https://linear.app/issues/65",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue");
        then.status(500);
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-65",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-65"));

    wait_for_path(&stub_dir.join("claude-args.txt"))?;
    wait_for_path(&stub_dir.join("prompt.txt"))?;
    wait_for_path(&stub_dir.join("provider-source.txt"))?;
    assert!(!stub_dir.join("codex.txt").exists());

    let args = fs::read_to_string(stub_dir.join("claude-args.txt"))?;
    assert!(args.contains("--permission-mode=bypassPermissions"));
    assert!(args.contains("--verbose"));
    assert!(args.contains("--output-format=stream-json"));
    assert!(args.contains("-p"));
    assert!(args.contains("--model=sonnet"));
    assert!(args.contains("--effort=high"));
    assert!(!args.contains("--reasoning="));
    assert_eq!(fs::read_to_string(stub_dir.join("agent.txt"))?, "claude");
    assert_eq!(fs::read_to_string(stub_dir.join("model.txt"))?, "sonnet");
    assert_eq!(fs::read_to_string(stub_dir.join("reasoning.txt"))?, "high");
    assert_eq!(
        fs::read_to_string(stub_dir.join("provider-source.txt"))?,
        "repo_default"
    );
    assert_eq!(
        fs::read_to_string(stub_dir.join("route-key.txt"))?,
        "agents.listen"
    );

    let state_path = listen_state_path(&config_path, &repo_root)?;
    let detail_path = listen_detail_path(&config_path, &repo_root, "MET-65")?;
    wait_for_json_pointer_value(&state_path, "/sessions/0/phase", &json!("blocked"))?;
    wait_for_json_pointer_value(&detail_path, "/phase", &json!("blocked"))?;
    wait_for_json_pointer_value(
        &state_path,
        "/sessions/0/canonical/provider",
        &json!("claude"),
    )?;
    wait_for_json_pointer_value(&state_path, "/sessions/0/canonical/model", &json!("sonnet"))?;
    wait_for_json_pointer_value(
        &state_path,
        "/sessions/0/canonical/reasoning",
        &json!("high"),
    )?;
    wait_for_json_pointer_value(&detail_path, "/canonical/provider", &json!("claude"))?;
    wait_for_json_pointer_value(&detail_path, "/canonical/model", &json!("sonnet"))?;
    wait_for_json_pointer_value(&detail_path, "/canonical/reasoning", &json!("high"))?;

    let state: serde_json::Value = serde_json::from_str(&fs::read_to_string(state_path)?)?;
    assert_eq!(
        state.pointer("/sessions/0/canonical/provider"),
        Some(&json!("claude"))
    );
    assert_eq!(
        state.pointer("/sessions/0/canonical/model"),
        Some(&json!("sonnet"))
    );
    assert_eq!(
        state.pointer("/sessions/0/canonical/reasoning"),
        Some(&json!("high"))
    );
    assert_eq!(state.pointer("/sessions/0/phase"), Some(&json!("blocked")));

    let detail: serde_json::Value = serde_json::from_str(&fs::read_to_string(detail_path)?)?;
    assert_eq!(detail.pointer("/phase"), Some(&json!("blocked")));
    assert_eq!(
        detail.pointer("/canonical/provider"),
        Some(&json!("claude"))
    );
    assert_eq!(detail.pointer("/canonical/model"), Some(&json!("sonnet")));
    assert_eq!(detail.pointer("/canonical/reasoning"), Some(&json!("high")));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_recreates_existing_workspace_clone_when_configured() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer",
    "refresh_policy": "recreate_from_origin_main"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;
    let stub_path = bin_dir.join("agent-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace_root = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-52")?;
    fs::write(workspace_root.join("stale.txt"), "remove me\n")?;
    let old_backlog_dir = workspace_root.join(format!("{}/backlog/MET-52", branding::PROJECT_DIR));
    fs::create_dir_all(&old_backlog_dir)?;
    fs::write(
        old_backlog_dir.join("index.md"),
        "# Old Backlog\n\nRemove me.\n",
    )?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-52",
                        "identifier": "MET-52",
                        "title": "Recreate existing listener workspace",
                        "description": "Recreate the local ticket workspace from origin/main",
                        "url": "https://linear.app/issues/52",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });
    let teams_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });
    let issue_detail_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-52\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-52",
                    "MET-52",
                    "Recreate existing listener workspace",
                    "Recreate the local ticket workspace from origin/main",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });
    let update_issue_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-52",
                        "identifier": "MET-52",
                        "title": "Recreate existing listener workspace",
                        "description": "Recreate the local ticket workspace from origin/main",
                        "url": "https://linear.app/issues/52",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });
    let create_comment_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-52",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-52"));

    assert!(viewer_mock.calls() >= 1);
    teams_mock.assert_calls(1);
    issue_detail_mock.assert_calls(1);
    update_issue_mock.assert_calls(1);
    create_comment_mock.assert_calls(1);

    wait_for_path(&stub_dir.join("payload.txt"))?;
    assert!(!workspace_root.join("stale.txt").exists());
    let recreated_backlog = fs::read_to_string(
        workspace_root.join(format!("{}/backlog/MET-52/index.md", branding::PROJECT_DIR)),
    )?;
    assert!(recreated_backlog.contains("## Requirements"));
    assert!(recreated_backlog.contains("Recreate the local ticket workspace from origin/main"));
    assert_eq!(
        git_stdout(&workspace_root, &["rev-parse", "--abbrev-ref", "HEAD"])?,
        "met-52-recreate-existing-listener-workspace"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_relaunches_agent_until_issue_leaves_active_states() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    for attempt in 1..=3 {
        let outcome = (|| -> Result<(), Box<dyn Error>> {
            let temp = tempdir()?;
            let repo_root = temp.path().join("repo");
            let config_path = temp.path().join("metastack.toml");
            let bin_dir = temp.path().join("bin");
            let stub_dir = temp.path().join("stub-output");
            let server = DynamicLinearServer::start_with_completion_after_refreshes(4)?;
            fs::create_dir_all(&repo_root)?;
            fs::create_dir_all(&bin_dir)?;
            fs::create_dir_all(&stub_dir)?;

            write_minimal_planning_context(
                &repo_root,
                r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer",
    "instructions_path": "instructions/listen.md"
  }
}
"#,
            )?;
            fs::create_dir_all(repo_root.join("instructions"))?;
            fs::write(
                repo_root.join("instructions/listen.md"),
                "# Listener Instructions\nKeep the workpad current.\n",
            )?;
            write_onboarded_config(
                &config_path,
                format!(
                    r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
                    api_url = server.url.as_str(),
                ),
            )?;
            let stub_path = bin_dir.join("agent-stub");
            fs::write(
                &stub_path,
                r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload-$count.txt"
printf '%s' "$METASTACK_AGENT_INSTRUCTIONS" > "$TEST_OUTPUT_DIR/instructions-$count.txt"
mkdir -p src
printf '// turn %s\n' "$count" > "src/turn-$count.rs"
"#,
            )?;
            let mut permissions = fs::metadata(&stub_path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&stub_path, permissions)?;
            write_listen_github_stub(
                &bin_dir.join("gh"),
                "none",
                "https://github.com/example/repo/pull/321",
            )?;
            init_repo_with_origin(&repo_root)?;

            let current_path = std::env::var("PATH")?;
            meta()
                .current_dir(&repo_root)
                .env("METASTACK_CONFIG", &config_path)
                .env("TEST_OUTPUT_DIR", &stub_dir)
                .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
                .args([
                    "listen",
                    "--root",
                    repo_root.to_str().expect("temp path should be utf-8"),
                    "--once",
                ])
                .assert()
                .success()
                .stdout(predicate::str::contains("1 claimed this cycle"))
                .stdout(predicate::str::contains("MET-32"));

            wait_for_path_with_timeout(&stub_dir.join("payload-2.txt"), Duration::from_secs(180))?;
            wait_for_path_with_timeout(
                &stub_dir.join("instructions-2.txt"),
                Duration::from_secs(180),
            )?;
            let turn_count = fs::read_to_string(stub_dir.join("count.txt"))?
                .trim()
                .parse::<u32>()?;
            assert!(
                turn_count >= 2,
                "expected at least two agent turns, observed {turn_count}"
            );

            let first_payload = fs::read_to_string(stub_dir.join("payload-1.txt"))?;
            let second_payload = fs::read_to_string(stub_dir.join("payload-2.txt"))?;
            let second_instructions = fs::read_to_string(stub_dir.join("instructions-2.txt"))?;
            assert!(!first_payload.contains("continuation turn #2 of 20"));
            assert!(
                second_payload.contains("continuation turn #2 of 20")
                    || second_payload.contains("continuation turn 2 of 20"),
                "unexpected second payload: {}",
                second_payload
            );
            assert!(second_instructions.contains("continuation turn 2 of 20"));

            let state_path = listen_state_path(&config_path, &repo_root)?;
            wait_for_file_substring_with_timeout(
                &state_path,
                "\"phase\": \"completed\"",
                Duration::from_secs(120),
            )?;
            let state = fs::read_to_string(state_path)?;
            assert!(state.contains("\"issue_identifier\": \"MET-32\""));
            assert!(state.contains("\"phase\": \"completed\""));
            assert!(state.contains("Human Review"));

            Ok(())
        })();

        match outcome {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 3 => {
                eprintln!(
                    "retrying listen_once_relaunches_agent_until_issue_leaves_active_states after attempt {attempt}: {error}"
                );
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("retry loop should return or surface the last failure")
}

#[cfg(unix)]
#[test]
fn listen_worker_writes_turn_token_summaries_and_persists_turn_history()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(4)?;
    let api_url = server.url.clone();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "claude"
"#,
        ),
    )?;

    let claude_path = bin_dir.join("claude");
    fs::write(
        &claude_path,
        r#"#!/bin/sh
set -eu
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-p, --print
--model <model>
--effort <level>
--verbose
--output-format <format>
--permission-mode <mode>
EOF
  exit 0
fi
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$count" > "$TEST_OUTPUT_DIR/turn-$count.txt"
mkdir -p src
printf '// turn %s\n' "$count" > "src/turn-$count.rs"
if [ "$count" -eq 1 ]; then
  input=210
  output=34
else
  input=80
  output=13
fi
printf '{"type":"message_start","message":{"usage":{"input_tokens":%s}}}\n' "$input"
printf '{"type":"message_delta","usage":{"output_tokens":%s}}\n' "$output"
printf '{"type":"result","subtype":"success","result":"claude listen ok","session_id":"listen-session-%s"}' "$count"
"#,
    )?;
    let mut permissions = fs::metadata(&claude_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&claude_path, permissions)?;
    write_listen_github_stub(
        &bin_dir.join("gh"),
        "none",
        "https://github.com/example/repo/pull/321",
    )?;

    init_repo_with_origin(&repo_root)?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env_remove("ANTHROPIC_API_KEY")
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success();

    wait_for_path_with_timeout(&stub_dir.join("turn-2.txt"), Duration::from_secs(180))?;
    let turn_count = fs::read_to_string(stub_dir.join("count.txt"))?
        .trim()
        .parse::<u32>()?;
    assert!(
        turn_count >= 2,
        "expected at least two turns, observed {turn_count}"
    );

    let state_path = listen_state_path(&config_path, &repo_root)?;
    wait_for_file_substring_with_timeout(
        &state_path,
        "\"phase\": \"completed\"",
        Duration::from_secs(180),
    )?;

    let log_path = listen_log_path(&config_path, &repo_root, "MET-32")?;
    let log = fs::read_to_string(&log_path)?;
    assert!(log.contains("turn 1 tokens: in 210 | out 34 | prompt_mode=full_prompt"));
    assert!(log.contains("turn 2 tokens: in 80 | out 13 | prompt_mode=continuation"));

    let detail_path = listen_detail_path(&config_path, &repo_root, "MET-32")?;
    let detail: serde_json::Value = serde_json::from_slice(&fs::read(&detail_path)?)?;
    let turn_history = detail["turn_history"]
        .as_array()
        .expect("turn_history should be an array");
    assert_eq!(turn_history.len(), 2);
    assert_eq!(turn_history[0]["turn"], 1);
    assert_eq!(turn_history[0]["prompt_mode"], "full_prompt");
    assert_eq!(turn_history[0]["tokens"]["input"], 210);
    assert_eq!(turn_history[0]["tokens"]["output"], 34);
    assert_eq!(turn_history[1]["turn"], 2);
    assert_eq!(turn_history[1]["prompt_mode"], "continuation");
    assert_eq!(turn_history[1]["tokens"]["input"], 80);
    assert_eq!(turn_history[1]["tokens"]["output"], 13);
    assert!(
        turn_history[0]["tokens"]["input"]
            .as_u64()
            .expect("turn 1 input tokens should be present")
            > turn_history[1]["tokens"]["input"]
                .as_u64()
                .expect("turn 2 input tokens should be present")
    );

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--turns",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Turn history:"))
        .stdout(predicate::str::contains(
            "turn 1 tokens: in 210 | out 34 | prompt_mode=full_prompt",
        ))
        .stdout(predicate::str::contains(
            "turn 2 tokens: in 80 | out 13 | prompt_mode=continuation",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_blocks_after_repeated_noop_turns() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer",
    "instructions_path": "instructions/listen.md"
  }
}
"#,
    )?;
    fs::create_dir_all(repo_root.join("instructions"))?;
    fs::write(
        repo_root.join("instructions/listen.md"),
        "# Listener Instructions\nKeep the workpad current.\n",
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    let stub_path = bin_dir.join("agent-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload-$count.txt"
printf '%s' "$METASTACK_AGENT_INSTRUCTIONS" > "$TEST_OUTPUT_DIR/instructions-$count.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;
    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let recipe_dir = workspace.join(format!("{}/verification/recipes", branding::PROJECT_DIR));
    fs::create_dir_all(&recipe_dir)?;
    fs::write(
        recipe_dir.join("agents.listen.yaml"),
        r#"quality_criteria:
  - Verification proof.
"#,
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [ ] Keep working\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--workspace",
            workspace.to_str().expect("workspace path should be utf-8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
        ])
        .assert()
        .success();

    let state_path = listen_state_path(&config_path, &repo_root)?;
    wait_for_file_substring(&state_path, "\"phase\": \"blocked\"")?;
    let turn_count = fs::read_to_string(stub_dir.join("count.txt"))?
        .trim()
        .parse::<u32>()?;
    assert!(
        turn_count >= 2,
        "expected at least two agent turns before the worker stalled, observed {turn_count}"
    );
    let state = fs::read_to_string(state_path)?;
    assert!(state.contains("\"issue_identifier\": \"MET-32\""));
    assert!(state.contains("\"phase\": \"blocked\""));
    assert!(state.contains("Blocked | stalled after 2 turn(s)"));
    assert!(state.contains("\"phase\": \"blocked\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_reuses_stored_provider_native_resume_handle() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let api_url = server.url.clone();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": [
      "if [ -f .codex-validation-ok ]; then exit 0; else touch .codex-validation-ok; exit 1; fi"
    ]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;

    let claude_path = bin_dir.join("claude");
    fs::write(
        &claude_path,
        r#"#!/bin/sh
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-p, --print
--model <model>
--effort <level>
--verbose
--output-format <format>
--permission-mode <mode>
EOF
  exit 0
fi
printf '%s\n' "$@" > "$TEST_OUTPUT_DIR/claude-args.txt"
printf '%s' '{"type":"result","subtype":"success","result":"claude listen ok","session_id":"provider-session-new"}'
"#,
    )?;
    let mut permissions = fs::metadata(&claude_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&claude_path, permissions)?;

    init_repo_with_origin(&repo_root)?;
    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "issue-32",
            "issue_identifier": "MET-32",
            "issue_title": "Resume worker from stored provider handle",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/MET-32",
            "phase": "blocked",
            "summary": "Waiting for worker retry",
            "brief_path": null,
            "workspace_path": workspace.display().to_string(),
            "workpad_comment_id": "comment-32",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": "legacy-session-should-not-be-used",
            "latest_resume_handle": {
                "provider": "claude",
                "id": "provider-resume-32"
            },
            "turns": 0,
            "tokens": {},
            "log_path": "logs/MET-32.log"
        })],
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env_remove("ANTHROPIC_API_KEY")
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--workspace",
            workspace.to_str().expect("workspace path should be utf-8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--agent",
            "claude",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let args = fs::read_to_string(stub_dir.join("claude-args.txt"))?;
    // Turn 1 must NOT pass --resume even when a stored handle exists (turn 2+ contract).
    assert!(!args.contains("--resume"));
    assert!(!args.contains("provider-resume-32"));
    assert!(!args.contains("legacy-session-should-not-be-used"));

    let state = fs::read_to_string(state_path)?;
    assert!(state.contains("\"latest_resume_handle\""));
    assert!(state.contains("\"provider\": \"claude\""));
    assert!(state.contains("\"id\": \"provider-session-new\""));
    assert!(state.contains("\"session_id\": \"provider-session-new\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_times_out_builtin_turns_and_records_timeout_reporting()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "claude"

[defaults.listen]
agent_turn_timeout_seconds = 1
agent_graceful_shutdown_seconds = 1
"#,
            api_url = server.url.as_str(),
        ),
    )?;

    let claude_path = bin_dir.join("claude");
    fs::write(
        &claude_path,
        r#"#!/bin/sh
set -eu
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-p, --print
--model <model>
--effort <level>
--verbose
--output-format <format>
--permission-mode <mode>
EOF
  exit 0
fi
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$count" > "$TEST_OUTPUT_DIR/turn-$count.txt"
printf '[{"type":"system","subtype":"init","session_id":"claude-timeout-session-%s"}]\n' "$count"
printf '{"type":"message_start","message":{"usage":{"input_tokens":%s}}}\n' "$((100 + count))"
printf '{"type":"message_delta","usage":{"output_tokens":%s}}\n' "$((10 + count))"
sleep 5
"#,
    )?;
    let mut permissions = fs::metadata(&claude_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&claude_path, permissions)?;
    write_listen_github_stub(
        &bin_dir.join("gh"),
        "none",
        "https://github.com/example/repo/pull/321",
    )?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-timeout-reporting";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn timeout() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare timeout reporting proof",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Timeout proof\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env_remove("ANTHROPIC_API_KEY")
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "2",
        ])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(stub_dir.join("count.txt"))?.trim(), "2");

    let log_path = listen_log_path(&config_path, &repo_root, "MET-32")?;
    let log = fs::read_to_string(&log_path)?;
    assert!(log.contains("turn timeout"));
    assert!(log.contains("turn=1"));
    assert!(log.contains("turn=2"));
    assert!(log.contains("timeout=1s"));
    assert!(log.contains("graceful_shutdown=1s"));
    assert!(!log.to_ascii_lowercase().contains("stalled"));

    let state_path = listen_state_path(&config_path, &repo_root)?;
    let state = fs::read_to_string(&state_path)?;
    assert!(state.contains("\"phase\": \"blocked\""));
    assert!(state.contains("turn 2 timeout"));
    assert!(state.contains("claude-timeout-session-2"));
    assert!(!state.to_ascii_lowercase().contains("stalled"));
    let state_json: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    assert_eq!(
        state_json["sessions"][0]["blocked"]["category"],
        json!("turn")
    );
    let state_blocked_reason = state_json["sessions"][0]["blocked"]["reason"]
        .as_str()
        .expect("blocked reason should be a string");
    assert!(state_blocked_reason.contains("turn 2 timeout | elapsed 1s | limit 1s | pid "));
    assert!(state_blocked_reason.ends_with(" | sigterm"));
    assert_eq!(
        state_json["sessions"][0]["blocked"]["retryable"],
        json!(false)
    );

    let detail_path = listen_detail_path(&config_path, &repo_root, "MET-32")?;
    let detail: serde_json::Value = serde_json::from_slice(&fs::read(&detail_path)?)?;
    assert_eq!(detail["phase"], json!("blocked"));
    assert_eq!(detail["blocked"]["category"], json!("turn"));
    let detail_blocked_reason = detail["blocked"]["reason"]
        .as_str()
        .expect("detail blocked reason should be a string");
    assert!(detail_blocked_reason.contains("turn 2 timeout | elapsed 1s | limit 1s | pid "));
    assert!(detail_blocked_reason.ends_with(" | sigterm"));
    assert_eq!(detail["blocked"]["retryable"], json!(false));
    assert_eq!(detail["last_timeout"]["turn"], json!(2));
    assert_eq!(detail["last_timeout"]["timeout_seconds"], json!(1));
    assert_eq!(
        detail["last_timeout"]["graceful_shutdown_seconds"],
        json!(1)
    );
    assert_eq!(detail["latest_resume_handle"]["provider"], json!("claude"));
    assert_eq!(
        detail["latest_resume_handle"]["id"],
        json!("claude-timeout-session-2")
    );
    assert_eq!(
        detail["tokens"]["input"].as_u64(),
        Some(203),
        "detail={detail}"
    );
    assert_eq!(
        detail["tokens"]["output"].as_u64(),
        Some(23),
        "detail={detail}"
    );
    assert_eq!(
        detail["turn_history"].as_array().map(Vec::len),
        Some(2),
        "detail={detail}"
    );
    assert!(!detail.to_string().to_ascii_lowercase().contains("stalled"));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["listen", "sessions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Turn Err"));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Phase: Turn Err"))
        .stdout(predicate::str::contains("Blocked category: Turn"))
        .stdout(predicate::str::contains("Detail blocked category: Turn"))
        .stdout(predicate::str::contains(
            "Detail last timeout: turn 2 timeout",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_logs_final_sigkill_termination_for_stubborn_builtin_timeouts()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "claude"

[defaults.listen]
agent_turn_timeout_seconds = 1
agent_graceful_shutdown_seconds = 1
"#,
            api_url = server.url.as_str(),
        ),
    )?;

    let claude_path = bin_dir.join("claude");
    fs::write(
        &claude_path,
        r#"#!/bin/sh
set -eu
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-p, --print
--model <model>
--effort <level>
--verbose
--output-format <format>
--permission-mode <mode>
EOF
  exit 0
fi
printf '[{"type":"system","subtype":"init","session_id":"claude-timeout-session-1"}]\n'
printf '{"type":"message_start","message":{"usage":{"input_tokens":101}}}\n'
printf '{"type":"message_delta","usage":{"output_tokens":11}}\n'
trap '' TERM
while :; do
  sleep 1
done
"#,
    )?;
    let mut permissions = fs::metadata(&claude_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&claude_path, permissions)?;
    write_listen_github_stub(
        &bin_dir.join("gh"),
        "none",
        "https://github.com/example/repo/pull/321",
    )?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-timeout-sigkill";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn timeout_sigkill() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare stubborn timeout reporting proof",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Timeout proof\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env_remove("ANTHROPIC_API_KEY")
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let log_path = listen_log_path(&config_path, &repo_root, "MET-32")?;
    let log = fs::read_to_string(&log_path)?;
    assert!(log.contains("turn timeout"));
    assert!(log.contains("turn=1"));
    assert!(log.contains("termination=sigkill"));

    let detail_path = listen_detail_path(&config_path, &repo_root, "MET-32")?;
    let detail: serde_json::Value = serde_json::from_slice(&fs::read(&detail_path)?)?;
    assert_eq!(detail["phase"], json!("blocked"));
    assert_eq!(detail["last_timeout"]["turn"], json!(1));
    assert_eq!(detail["last_timeout"]["termination"], json!("sigkill"));
    assert_eq!(
        detail["latest_resume_handle"]["id"],
        json!("claude-timeout-session-1")
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_reuses_stored_codex_resume_handle() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let home_dir = temp.path().join("home");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let api_url = server.url.clone();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&home_dir)?;
    fs::create_dir_all(&stub_dir)?;
    fs::create_dir_all(home_dir.join(".codex"))?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    fs::write(
        home_dir.join(".codex/config.toml"),
        r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
"#,
    )?;

    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  cat <<'EOF'
-a, --ask-for-approval <APPROVAL_POLICY>
-s, --sandbox <SANDBOX_MODE>
-C, --cd <DIR>
    --add-dir <DIR>
    --dangerously-bypass-approvals-and-sandbox
EOF
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-m, --model <MODEL>
-c, --config <key=value>
    --json
EOF
  exit 0
fi
printf '%s\n' "$@" > "$TEST_OUTPUT_DIR/codex-args.txt"
printf '%s\n' '{"type":"thread.started","thread_id":"provider-thread-new"}'
printf '%s' '{"type":"item.completed","item":{"type":"agent_message","text":"{\"summary\":\"codex listen ok\"}"}}'
"#,
    )?;
    let mut permissions = fs::metadata(&codex_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&codex_path, permissions)?;

    init_repo_with_origin(&repo_root)?;
    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "issue-32",
            "issue_identifier": "MET-32",
            "issue_title": "Resume codex worker from stored provider handle",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/MET-32",
            "phase": "blocked",
            "summary": "Waiting for worker retry",
            "brief_path": null,
            "workspace_path": workspace.display().to_string(),
            "workpad_comment_id": "comment-32",
            "updated_at_epoch_seconds": 1_773_575_101u64,
            "pid": null,
            "session_id": "legacy-session-should-not-be-used",
            "latest_resume_handle": {
                "provider": "codex",
                "id": "provider-thread-32"
            },
            "turns": 0,
            "tokens": {},
            "log_path": "logs/MET-32.log"
        })],
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("HOME", &home_dir)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--workspace",
            workspace.to_str().expect("workspace path should be utf-8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--agent",
            "codex",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let args_path = stub_dir.join("codex-args.txt");
    wait_for_path(&args_path)?;
    let args = fs::read_to_string(args_path)?;
    // Turn 1 must NOT pass resume even when a stored handle exists (turn 2+ contract).
    assert!(!args.contains("provider-thread-32"));
    assert!(!args.contains("legacy-session-should-not-be-used"));

    wait_for_path(&state_path)?;
    let state = fs::read_to_string(state_path)?;
    assert!(state.contains("\"latest_resume_handle\""));
    assert!(state.contains("\"provider\": \"codex\""));
    assert!(state.contains("\"id\": \"provider-thread-new\""));
    assert!(state.contains("\"session_id\": \"provider-thread-new\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_publishes_the_initial_branch_pull_request_as_a_draft() -> Result<(), Box<dyn Error>>
{
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(
        bin_dir.join("agent-stub"),
        r#"#!/bin/sh
mkdir -p src
printf 'pub fn turn_one() {}\n' > src/turn-one.rs
"#,
    )?;
    write_listen_github_stub(
        &bin_dir.join("gh"),
        "none",
        "https://github.com/example/repo/pull/321",
    )?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    write_minimal_planning_context(
        &workspace,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    let branch = "met-32-continuation-loop";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn initial() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare draft PR publication",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [ ] Keep working\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert!(gh_log.contains("pr list --state open --head met-32-continuation-loop --base main"));
    assert!(gh_log.contains("pr create --base main --head met-32-continuation-loop"));
    assert!(gh_log.contains("--draft --json number,url,isDraft"));
    assert!(!gh_log.contains("pr ready 321"));
    let pr_body = fs::read_to_string(workspace.join(format!(
        "{}/agents/MET-32-pull-request.md",
        branding::PROJECT_DIR
    )))?;
    assert!(pr_body.contains("## Summary"));
    assert!(pr_body.contains("Latest listener review:"));
    assert!(pr_body.contains("## Completed In This Branch"));
    assert!(pr_body.contains("Changed `?? src/turn-one.rs`"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_publishes_a_pull_request_after_push_without_a_local_remote_tracking_ref()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(
        repo_root.join(".gitignore"),
        format!("{}\n", branding::PROJECT_DIR),
    )?;
    fs::write(bin_dir.join("agent-stub"), "#!/bin/sh\n:\n")?;
    write_listen_github_stub(
        &bin_dir.join("gh"),
        "missing",
        "https://github.com/example/repo/pull/321",
    )?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    write_minimal_planning_context(
        &workspace,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    let branch = "met-32-continuation-loop";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn draft() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare missing remote tracking ref",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "update-ref",
            "-d",
            &format!("refs/remotes/origin/{branch}"),
        ])
        .status()?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [ ] Keep working\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert!(gh_log.contains("pr list --state open --head met-32-continuation-loop --base main"));
    assert!(gh_log.contains("pr create --base main --head met-32-continuation-loop"));
    let pr_body = fs::read_to_string(workspace.join(format!(
        "{}/agents/MET-32-pull-request.md",
        branding::PROJECT_DIR
    )))?;
    assert!(pr_body.contains("Latest listener review:"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_promotes_the_same_draft_pull_request_during_review_handoff()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(
        repo_root.join(".gitignore"),
        format!("{}\n", branding::PROJECT_DIR),
    )?;
    fs::write(bin_dir.join("agent-stub"), "#!/bin/sh\n:\n")?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    write_minimal_planning_context(
        &workspace,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    let branch = "met-32-continuation-loop";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn ready() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare ready promotion",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "draft",
        "https://github.com/example/repo/pull/321",
    )?;
    // Keep an uncommitted change so review handoff auto-clean skips and the stored session
    // remains inspectable for this PR-promotion assertion.
    fs::write(workspace.join("dirty-skip.txt"), "local review note\n")?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert!(gh_log.contains("pr edit 321 --title MET-32: Continuation loop --body-file"));
    assert!(gh_log.contains("pr ready 321"));
    assert!(!gh_log.contains("pr create --base main --head met-32-continuation-loop"));
    assert!(
        workspace.is_dir(),
        "dirty workspace should be kept for manual review"
    );
    let state_path = listen_state_path(&config_path, &repo_root)?;
    wait_for_file_substring(&state_path, "\"phase\": \"completed\"")?;
    let state = fs::read_to_string(&state_path)?;
    assert!(state.contains("\"phase\": \"completed\""));
    assert!(state.contains("Human Review"));
    assert!(state.contains("\"status\": \"ready\""));

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "sessions",
            "inspect",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ready #321"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_replays_pending_review_transition_after_linear_recovery()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let failing_server = DynamicLinearServer::start_with_failure_plan(
        1_000_000,
        DynamicLinearFailurePlan {
            review_transition_failures: 3,
            ..DynamicLinearFailurePlan::default()
        },
    )?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = failing_server.url.as_str(),
        ),
    )?;
    fs::write(bin_dir.join("agent-stub"), "#!/bin/sh\n:\n")?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    write_minimal_planning_context(
        &workspace,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    let branch = "met-32-replay-review-transition";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn review_transition() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare review transition replay",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "none",
        "https://github.com/example/repo/pull/321",
    )?;
    fs::write(
        workspace.join("dirty-keep.txt"),
        "keep workspace for replay assertions\n",
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let state_path = listen_state_path(&config_path, &repo_root)?;
    let first_state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    assert_eq!(first_state["sessions"][0]["phase"], json!("blocked"));
    assert_eq!(
        first_state["sessions"][0]["pending_linear_sync"]["review_transition_issue"],
        json!(true)
    );
    assert_eq!(
        first_state["sessions"][0]["pull_request"]["status"],
        json!("ready")
    );

    drop(failing_server);
    let recovered_server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
            api_url = recovered_server.url.as_str(),
        ),
    )?;

    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let second_state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    assert_eq!(second_state["sessions"][0]["phase"], json!("completed"));
    assert!(
        second_state["sessions"][0]
            .get("pending_linear_sync")
            .is_none()
            || second_state["sessions"][0]["pending_linear_sync"].is_null()
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_auto_cleans_safe_workspace_during_review_handoff() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(
        repo_root.join(".gitignore"),
        format!("{}\n", branding::PROJECT_DIR),
    )?;
    fs::write(bin_dir.join("agent-stub"), "#!/bin/sh\n:\n")?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    write_minimal_planning_context(
        &workspace,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    let branch = "met-32-continuation-loop";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn ready() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare ready promotion",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "draft",
        "https://github.com/example/repo/pull/321",
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert!(gh_log.contains("pr edit 321 --title MET-32: Continuation loop --body-file"));
    assert!(gh_log.contains("pr ready 321"));
    assert!(
        !workspace.exists(),
        "safe workspace should be auto-cleaned after review handoff completion"
    );

    let state_path = listen_state_path(&config_path, &repo_root)?;
    let state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let sessions = state["sessions"]
        .as_array()
        .expect("sessions should remain an array");
    assert!(
        sessions.is_empty(),
        "auto-clean should remove the completed ticket session entry"
    );
    assert!(
        !listen_log_path(&config_path, &repo_root, "MET-32")?.exists(),
        "ticket log should be removed during auto-clean"
    );
    assert!(
        !listen_detail_path(&config_path, &repo_root, "MET-32")?.exists(),
        "ticket detail should be removed during auto-clean"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_render_once_auto_cleans_completed_listener_workspace_after_merge()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-post-merge-cleanup";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn merged_cleanup() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare post-merge cleanup workspace",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;

    fs::write(
        bin_dir.join("gh"),
        format!(
            r#"#!/bin/sh
set -eu
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '%s' '[{{"headRefName":"{branch}","state":"MERGED"}}]'
  exit 0
fi
printf 'unexpected gh invocation: %s\n' "$*" >&2
exit 1
"#
        ),
    )?;
    let mut permissions = fs::metadata(bin_dir.join("gh"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("gh"), permissions)?;

    let _viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let _issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-32",
                        "identifier": "MET-32",
                        "title": "Later cleanup",
                        "description": "Preserve dirty completed listener workspace after merge",
                        "url": "https://linear.app/issues/MET-32",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-done",
                            "name": "Done",
                            "type": "completed"
                        }
                    }],
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-32\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-32",
                    "MET-32",
                    "Later cleanup",
                    "Preserve dirty completed listener workspace after merge",
                    "state-done",
                    "Done",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut session = listen_session_json("MET-32", "completed", now, None);
    session["summary"] = json!("Completed | waiting for merge reconciliation");
    session["workspace_path"] = json!(workspace.display().to_string());
    session["branch"] = json!(branch);
    session["origin"] = json!("listen");
    session["started_at_epoch_seconds"] = json!(now.saturating_sub(60));
    session["stale_worker_recovery_attempt_count"] = json!(0);
    write_listen_store_session(&config_path, &repo_root, vec![session])?;

    let log_path = listen_log_path(&config_path, &repo_root, "MET-32")?;
    fs::create_dir_all(log_path.parent().expect("log path should have a parent"))?;
    fs::write(&log_path, "log for MET-32\n")?;
    let detail_path = listen_detail_path(&config_path, &repo_root, "MET-32")?;
    fs::create_dir_all(
        detail_path
            .parent()
            .expect("detail path should have a parent"),
    )?;
    fs::write(&detail_path, "{\"phase\":\"completed\"}\n")?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--render-once",
            "--width",
            "160",
            "--height",
            "48",
            "--root",
            repo_root.to_str().expect("utf8"),
        ])
        .assert()
        .success();

    assert!(
        !workspace.exists(),
        "clean completed listener workspace should be removed after merge reconciliation"
    );

    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(listen_state_path(&config_path, &repo_root)?)?)?;
    let sessions = state["sessions"]
        .as_array()
        .expect("sessions should remain an array");
    assert!(
        sessions.is_empty(),
        "auto-clean should remove the completed session after merge reconciliation"
    );
    assert!(
        !log_path.exists(),
        "ticket log should be removed during later auto-clean"
    );
    assert!(
        !detail_path.exists(),
        "ticket detail should be removed during later auto-clean"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_render_once_preserves_dirty_completed_listener_workspace_after_merge()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-post-merge-preserve";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn preserved_cleanup() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare preserved post-merge workspace",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    fs::write(workspace.join("dirty-note.txt"), "keep this workspace\n")?;

    fs::write(
        bin_dir.join("gh"),
        format!(
            r#"#!/bin/sh
set -eu
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '%s' '[{{"headRefName":"{branch}","state":"MERGED"}}]'
  exit 0
fi
printf 'unexpected gh invocation: %s\n' "$*" >&2
exit 1
"#
        ),
    )?;
    let mut permissions = fs::metadata(bin_dir.join("gh"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("gh"), permissions)?;

    let _viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let _issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-32",
                        "identifier": "MET-32",
                        "title": "Later cleanup",
                        "description": "Preserve dirty completed listener workspace after merge",
                        "url": "https://linear.app/issues/MET-32",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-done",
                            "name": "Done",
                            "type": "completed"
                        }
                    }],
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-32\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-32",
                    "MET-32",
                    "Later cleanup",
                    "Preserve dirty completed listener workspace after merge",
                    "state-done",
                    "Done",
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
        }));
    });

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut session = listen_session_json("MET-32", "completed", now, None);
    session["summary"] = json!("Completed | waiting for merge reconciliation");
    session["workspace_path"] = json!(workspace.display().to_string());
    session["branch"] = json!(branch);
    session["origin"] = json!("listen");
    session["started_at_epoch_seconds"] = json!(now.saturating_sub(60));
    session["stale_worker_recovery_attempt_count"] = json!(0);
    write_listen_store_session(&config_path, &repo_root, vec![session])?;

    let log_path = listen_log_path(&config_path, &repo_root, "MET-32")?;
    fs::create_dir_all(log_path.parent().expect("log path should have a parent"))?;
    fs::write(&log_path, "log for MET-32\n")?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--render-once",
            "--width",
            "160",
            "--height",
            "48",
            "--root",
            repo_root.to_str().expect("utf8"),
        ])
        .assert()
        .success();

    assert!(
        workspace.exists(),
        "dirty completed listener workspace should be preserved after merge reconciliation"
    );
    assert!(
        log_path.exists(),
        "preserved workspaces should retain ticket log artifacts"
    );

    let state_text = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(state_text.contains("preserved after merge"));
    assert!(state_text.contains("uncommitted changes detected"));

    let inspect = inspect_listen_sessions(&repo_root, &config_path)?;
    assert!(inspect.contains("preserved after merge"));
    assert!(inspect.contains("uncommitted changes detected"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_leaves_an_already_ready_pull_request_unchanged_on_continuation()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(bin_dir.join("agent-stub"), "#!/bin/sh\n:\n")?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-continuation-loop";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn already_ready() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Continue with ready PR",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "ready",
        "https://github.com/example/repo/pull/321",
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert!(gh_log.contains("pr edit 321 --title MET-32: Continuation loop --body-file"));
    assert!(!gh_log.contains("pr create --base main --head met-32-continuation-loop"));
    assert!(!gh_log.contains("pr ready 321"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_handles_a_missing_matching_pull_request_during_review_handoff()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(bin_dir.join("agent-stub"), "#!/bin/sh\n:\n")?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-continuation-loop";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn no_pr() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Complete without an open PR",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "none",
        "https://github.com/example/repo/pull/321",
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert!(gh_log.contains("pr list --state open --head met-32-continuation-loop --base main"));
    assert!(gh_log.contains("pr create --base main --head met-32-continuation-loop"));
    assert!(gh_log.contains("pr ready 321"));

    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(state.contains("\"phase\": \"completed\""));
    assert!(state.contains("\"status\": \"ready\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_blocks_when_github_pull_request_stays_draft_after_ready_handoff()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(bin_dir.join("agent-stub"), "#!/bin/sh\n:\n")?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-continuation-loop";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn stubborn() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare stubborn ready promotion",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "stubborn-draft",
        "https://github.com/example/repo/pull/321",
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "1",
        ])
        .assert()
        .failure();

    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert!(gh_log.contains("pr ready 321"));
    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(state.contains("\"phase\": \"blocked\""));
    assert!(!state.contains("\"status\": \"ready\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_retries_failed_pre_pr_validation_and_blocks_when_budget_is_exhausted()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["sh -lc 'count_file=\"$TEST_OUTPUT_DIR/validation-count.txt\"; count=0; [ -f \"$count_file\" ] && count=$(cat \"$count_file\"); count=$((count + 1)); printf \"%s\" \"$count\" > \"$count_file\"; test -f repaired.txt'"],
    "repair_attempts": 1,
    "profile": "pre-pr-gate"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(
        bin_dir.join("agent-stub"),
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload-$count.txt"
"#,
    )?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-validation-block";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn gate() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare validation gate proof",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "none",
        "https://github.com/example/repo/pull/321",
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "2",
        ])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(stub_dir.join("count.txt"))?.trim(), "2");
    assert_eq!(
        fs::read_to_string(stub_dir.join("validation-count.txt"))?.trim(),
        "2"
    );
    assert!(
        fs::read_to_string(stub_dir.join("payload-2.txt"))?
            .contains("Repair the local validation failure and rerun the validation gate before"),
    );
    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(state.contains("\"phase\": \"blocked\""));
    assert!(state.contains("validation failed and repair budget exhausted"));
    let gh_log_path = stub_dir.join("gh.log");
    if gh_log_path.exists() {
        let gh_log = fs::read_to_string(gh_log_path)?;
        assert!(gh_log.contains("pr create --base main --head met-32-validation-block"));
        assert!(!gh_log.contains("pr ready 321"));
    }

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_repairs_failing_pr_checks_and_reuses_the_same_pull_request()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["sh -lc 'count_file=\"$TEST_OUTPUT_DIR/validation-count.txt\"; count=0; [ -f \"$count_file\" ] && count=$(cat \"$count_file\"); count=$((count + 1)); printf \"%s\" \"$count\" > \"$count_file\"; test -f repaired.txt'"],
    "repair_attempts": 2,
    "profile": "ci-repair"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
code_review = false
"#,
            api_url = server.url.as_str(),
        ),
    )?;
    fs::write(
        bin_dir.join("agent-stub"),
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload-$count.txt"
printf '%s' 'ok' > repaired.txt
"#,
    )?;
    let mut permissions = fs::metadata(bin_dir.join("agent-stub"))?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin_dir.join("agent-stub"), permissions)?;
    init_repo_with_origin(&repo_root)?;

    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let branch = "met-32-ci-repair";
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "checkout",
            "-B",
            branch,
            "main",
        ])
        .status()?;
    fs::write(workspace.join("src.rs"), "pub fn ready() {}\n")?;
    ProcessCommand::new("git")
        .args(["-C", workspace.to_str().expect("utf8"), "add", "src.rs"])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "commit",
            "-m",
            "Prepare CI repair proof",
        ])
        .status()?;
    ProcessCommand::new("git")
        .args([
            "-C",
            workspace.to_str().expect("utf8"),
            "push",
            "--set-upstream",
            "origin",
            branch,
        ])
        .status()?;
    write_listen_github_stub_with_checks_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "none",
        "https://github.com/example/repo/pull/321",
        "fail-once",
    )?;
    fs::write(
        workspace.join("dirty-skip.txt"),
        "keep workspace for assertions\n",
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Complete\n",
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--max-turns",
            "2",
        ])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(stub_dir.join("count.txt"))?.trim(), "2");
    assert_eq!(
        fs::read_to_string(stub_dir.join("validation-count.txt"))?.trim(),
        "2"
    );
    assert!(
        fs::read_to_string(stub_dir.join("payload-2.txt"))?
            .contains("Repair failing GitHub checks on PR #321 and update the same PR."),
    );
    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert_eq!(
        gh_log
            .matches("pr create --base main --head met-32-ci-repair")
            .count(),
        1
    );
    assert!(gh_log.contains("pr edit 321 --title MET-32: Continuation loop --body-file"));
    assert!(gh_log.contains("pr checks 321 --json name,state,bucket,description,link"));
    assert!(gh_log.contains("pr edit 321 --add-label metastack"));
    assert_eq!(
        fs::read_to_string(stub_dir.join("gh-checks-count.txt"))?.trim(),
        "2"
    );

    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(state.contains("\"phase\": \"completed\""));
    assert!(state.contains("\"number\": 321"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_skips_ineligible_issue_and_records_the_reason() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-31",
                        "identifier": "MET-31",
                        "title": "Ignored work",
                        "description": "Should not be claimed",
                        "url": "https://linear.app/issues/31",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-2",
                            "name": "Someone Else",
                            "email": "else@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "manual"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });

    let update_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Watching: Kames + unassigned"))
        .stdout(predicate::str::contains("MET-31").not());

    assert!(viewer_mock.calls() >= 1);
    update_mock.assert_calls(0);
    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(!state.contains("MET-31"));
    assert!(!temp.path().join("repo-workspace/MET-31").exists());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_claims_viewer_assigned_issue_in_viewer_only_scope() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer_only"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;
    let stub_path = bin_dir.join("agent-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-54",
                        "identifier": "MET-54",
                        "title": "Claim viewer assigned work",
                        "description": "Viewer-only scope should still claim viewer work",
                        "url": "https://linear.app/issues/54",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-1",
                            "name": "Kames",
                            "email": "sudo@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-54\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": {
                    "id": "issue-54",
                    "identifier": "MET-54",
                    "title": "Claim viewer assigned work",
                    "description": "Viewer-only scope should still claim viewer work",
                    "url": "https://linear.app/issues/MET-54",
                    "priority": 2,
                    "updatedAt": "2026-03-14T16:00:00Z",
                    "team": {
                        "id": "team-1",
                        "key": "MET",
                        "name": "Metastack"
                    },
                    "project": {
                        "id": "project-1",
                        "name": "MetaStack CLI"
                    },
                    "assignee": {
                        "id": "viewer-1",
                        "name": "Kames",
                        "email": "sudo@example.com"
                    },
                    "labels": {
                        "nodes": [{
                            "id": "label-1",
                            "name": "agent"
                        }]
                    },
                    "comments": { "nodes": [] },
                    "state": {
                        "id": "state-2",
                        "name": "In Progress",
                        "type": "started"
                    },
                    "attachments": { "nodes": [] },
                    "parent": null,
                    "children": { "nodes": [] }
                }
            }
        }));
    });
    let update_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-54",
                        "identifier": "MET-54",
                        "title": "Claim viewer assigned work",
                        "description": "Viewer-only scope should still claim viewer work",
                        "url": "https://linear.app/issues/54",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue");
        then.status(500);
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-54",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    let state_path = listen_state_path(&config_path, &repo_root)?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Watching: only Kames"))
        .stdout(predicate::str::contains("1 claimed this cycle"))
        .stdout(predicate::str::contains("MET-54"));

    assert!(viewer_mock.calls() >= 1);
    assert!(issues_mock.calls() >= 3);
    update_mock.assert_calls(1);
    assert!(temp.path().join("repo-workspace/MET-54").is_dir());
    let state = fs::read_to_string(state_path)?;
    assert!(state.contains("\"issue_identifier\": \"MET-54\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_skips_unassigned_issue_in_viewer_only_scope() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer_only"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-55",
                        "identifier": "MET-55",
                        "title": "Ignore unassigned strict-mode work",
                        "description": "Viewer-only scope should skip unassigned work",
                        "url": "https://linear.app/issues/55",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": null,
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });
    let update_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Watching: only Kames"))
        .stdout(predicate::str::contains("MET-55").not());

    assert!(viewer_mock.calls() >= 1);
    update_mock.assert_calls(0);
    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(!state.contains("MET-55"));
    assert!(!temp.path().join("repo-workspace/MET-55").exists());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_skips_foreign_assigned_issue_in_viewer_only_scope() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer_only"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-56",
                        "identifier": "MET-56",
                        "title": "Ignore foreign strict-mode work",
                        "description": "Viewer-only scope should skip someone else's work",
                        "url": "https://linear.app/issues/56",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-2",
                            "name": "Someone Else",
                            "email": "else@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });
    let update_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Watching: only Kames"))
        .stdout(predicate::str::contains("MET-56").not());

    assert!(viewer_mock.calls() >= 1);
    update_mock.assert_calls(0);
    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(!state.contains("MET-56"));
    assert!(!temp.path().join("repo-workspace/MET-56").exists());

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_claims_unassigned_issue_in_viewer_scope() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;
    let stub_path = bin_dir.join("agent-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;

    let viewer_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-52",
                        "identifier": "MET-52",
                        "title": "Claim unassigned listen work",
                        "description": "Unassigned issues should now be eligible",
                        "url": "https://linear.app/issues/52",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": null,
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-52\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": {
                    "id": "issue-52",
                    "identifier": "MET-52",
                    "title": "Claim unassigned listen work",
                    "description": "Unassigned issues should now be eligible",
                    "url": "https://linear.app/issues/MET-52",
                    "priority": 2,
                    "updatedAt": "2026-03-14T16:00:00Z",
                    "team": {
                        "id": "team-1",
                        "key": "MET",
                        "name": "Metastack"
                    },
                    "project": {
                        "id": "project-1",
                        "name": "MetaStack CLI"
                    },
                    "assignee": null,
                    "labels": {
                        "nodes": [{
                            "id": "label-1",
                            "name": "agent"
                        }]
                    },
                    "comments": { "nodes": [] },
                    "state": {
                        "id": "state-2",
                        "name": "In Progress",
                        "type": "started"
                    },
                    "attachments": { "nodes": [] },
                    "parent": null,
                    "children": { "nodes": [] }
                }
            }
        }));
    });
    let update_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-52",
                        "identifier": "MET-52",
                        "title": "Claim unassigned listen work",
                        "description": "Unassigned issues should now be eligible",
                        "url": "https://linear.app/issues/52",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue");
        then.status(500);
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-52",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    let state_path = listen_state_path(&config_path, &repo_root)?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Watching: Kames + unassigned"))
        .stdout(predicate::str::contains("1 claimed this cycle"))
        .stdout(predicate::str::contains("MET-52"));

    assert!(viewer_mock.calls() >= 1);
    assert!(issues_mock.calls() >= 3);
    update_mock.assert_calls(1);
    assert!(temp.path().join("repo-workspace/MET-52").is_dir());
    let state = fs::read_to_string(state_path)?;
    assert!(state.contains("\"issue_identifier\": \"MET-52\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_once_all_assignees_override_claims_foreign_assigned_issue_without_changing_repo_scope()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "required_label": "agent",
    "assignment_scope": "viewer"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "agent-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;
    let stub_path = bin_dir.join("agent-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload.txt"
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;
    init_repo_with_origin(&repo_root)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Viewer");
        then.status(200).json_body(json!({
            "data": {
                "viewer": {
                    "id": "viewer-1",
                    "name": "Kames",
                    "email": "sudo@example.com"
                }
            }
        }));
    });
    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-53",
                        "identifier": "MET-53",
                        "title": "Claim foreign assigned work with override",
                        "description": "Run-scoped override should allow pickup",
                        "url": "https://linear.app/issues/53",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:00:00Z",
                        "assignee": {
                            "id": "viewer-2",
                            "name": "Someone Else",
                            "email": "else@example.com"
                        },
                        "labels": {
                            "nodes": [{
                                "id": "label-1",
                                "name": "agent"
                            }]
                        },
                        "comments": {
                            "nodes": []
                        },
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-1",
                            "name": "Todo",
                            "type": "unstarted"
                        }
                    }]
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)")
            .body_includes("\"id\":\"issue-53\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": {
                    "id": "issue-53",
                    "identifier": "MET-53",
                    "title": "Claim foreign assigned work with override",
                    "description": "Run-scoped override should allow pickup",
                    "url": "https://linear.app/issues/MET-53",
                    "priority": 2,
                    "updatedAt": "2026-03-14T16:00:00Z",
                    "team": {
                        "id": "team-1",
                        "key": "MET",
                        "name": "Metastack"
                    },
                    "project": {
                        "id": "project-1",
                        "name": "MetaStack CLI"
                    },
                    "assignee": {
                        "id": "viewer-2",
                        "name": "Someone Else",
                        "email": "else@example.com"
                    },
                    "labels": {
                        "nodes": [{
                            "id": "label-1",
                            "name": "agent"
                        }]
                    },
                    "comments": { "nodes": [] },
                    "state": {
                        "id": "state-2",
                        "name": "In Progress",
                        "type": "started"
                    },
                    "attachments": { "nodes": [] },
                    "parent": null,
                    "children": { "nodes": [] }
                }
            }
        }));
    });
    let update_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": {
                        "id": "issue-53",
                        "identifier": "MET-53",
                        "title": "Claim foreign assigned work with override",
                        "description": "Run-scoped override should allow pickup",
                        "url": "https://linear.app/issues/53",
                        "priority": 2,
                        "updatedAt": "2026-03-14T16:05:00Z",
                        "team": {
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        },
                        "project": {
                            "id": "project-1",
                            "name": "MetaStack CLI"
                        },
                        "state": {
                            "id": "state-2",
                            "name": "In Progress",
                            "type": "started"
                        }
                    }
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue");
        then.status(500);
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment")
            .body_includes("## Codex Workpad");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-53",
                        "body": "## Codex Workpad",
                        "resolvedAt": null
                    }
                }
            }
        }));
    });

    let current_path = std::env::var("PATH")?;
    let state_path = listen_state_path(&config_path, &repo_root)?;
    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--once",
            "--all-assignees",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Watching: all assignees"))
        .stdout(predicate::str::contains("1 claimed this cycle"))
        .stdout(predicate::str::contains("MET-53"));

    assert!(issues_mock.calls() >= 3);
    update_mock.assert_calls(1);
    assert!(temp.path().join("repo-workspace/MET-53").is_dir());
    let state = fs::read_to_string(state_path)?;
    assert!(state.contains("\"issue_identifier\": \"MET-53\""));
    assert!(
        fs::read_to_string(repo_root.join(format!("{}/meta.json", branding::PROJECT_DIR)))?
            .contains("\"assignment_scope\": \"viewer\"")
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Session resume: continuation prompt on turn 2+ (ENG-10303)
// ---------------------------------------------------------------------------

/// Validates that a Claude listen worker uses the continuation prompt on turn 2
/// when a resume handle is captured from turn 1. Turn 1 should receive the full
/// prompt and instructions. Turn 2 should receive a compact continuation prompt
/// with no instructions, and the CLI args should include `--resume <session_id>`.
#[cfg(unix)]
#[test]
fn listen_worker_claude_uses_continuation_prompt_on_resumed_turn() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let api_url = server.url.clone();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;

    // Stub claude binary that captures the resumed-turn prompt, instructions, and args.
    let claude_path = bin_dir.join("claude");
    fs::write(
        &claude_path,
        r#"#!/bin/sh
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-p, --print
--model <model>
--effort <level>
--verbose
--output-format <format>
--permission-mode <mode>
--resume <session_id>
EOF
  exit 0
fi
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s\n' "$@" > "$TEST_OUTPUT_DIR/claude-args-$count.txt"
printf '%s' "$METASTACK_AGENT_PROMPT" > "$TEST_OUTPUT_DIR/prompt-$count.txt"
printf '%s' "$METASTACK_AGENT_INSTRUCTIONS" > "$TEST_OUTPUT_DIR/instructions-$count.txt"
mkdir -p src
printf '// turn %s\n' "$count" > "src/turn-$count.rs"
printf '%s\n' '{"type":"message_start","message":{"usage":{"input_tokens":210}}}'
printf '%s\n' '{"type":"message_delta","usage":{"output_tokens":34}}'
printf '%s' '{"type":"result","subtype":"success","result":"claude listen ok","session_id":"claude-session-resume-1"}'
"#,
    )?;
    let mut permissions = fs::metadata(&claude_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&claude_path, permissions)?;

    init_repo_with_origin(&repo_root)?;
    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;

    // Start with a stored resume handle so the resumed-turn path is exercised
    // deterministically in a single worker invocation.
    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "issue-32",
            "issue_identifier": "MET-32",
            "issue_title": "Session resume continuation test",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/MET-32",
            "phase": "blocked",
            "summary": "Waiting for Claude continuation turn",
            "brief_path": null,
            "workspace_path": workspace.display().to_string(),
            "workpad_comment_id": "comment-32",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": "claude-session-resume-1",
            "latest_resume_handle": {
                "provider": "claude",
                "id": "claude-session-resume-1"
            },
            "turns": 1,
            "tokens": {},
            "canonical": {},
            "log_path": "logs/MET-32.log"
        })],
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env_remove("ANTHROPIC_API_KEY")
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--workspace",
            workspace.to_str().expect("workspace path should be utf-8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--agent",
            "claude",
            "--max-turns",
            "2",
        ])
        .assert()
        .success();

    // Verify the resumed turn ran exactly once.
    let turn_count = fs::read_to_string(stub_dir.join("count.txt"))?
        .trim()
        .parse::<u32>()?;
    assert_eq!(turn_count, 1, "expected exactly one resumed agent turn");

    let prompt_1 = fs::read_to_string(stub_dir.join("prompt-1.txt"))?;
    let instructions_1 = fs::read_to_string(stub_dir.join("instructions-1.txt"))?;
    assert!(
        prompt_1.contains("Continuation guidance"),
        "resumed turn should receive continuation prompt, got: {}",
        &prompt_1[..prompt_1.len().min(200)]
    );
    assert!(
        !prompt_1.contains("You are working on Linear ticket"),
        "resumed turn should not include full issue context"
    );
    assert!(
        instructions_1.is_empty(),
        "resumed turn should have empty instructions on resume, got: {}",
        &instructions_1[..instructions_1.len().min(200)]
    );

    let args_1 = fs::read_to_string(stub_dir.join("claude-args-1.txt"))?;
    assert!(
        args_1.contains("--resume"),
        "resumed turn should have --resume flag"
    );
    assert!(
        args_1.contains("claude-session-resume-1"),
        "resumed turn should reuse the stored session id"
    );

    let state = fs::read_to_string(state_path)?;
    assert!(state.contains("\"id\": \"claude-session-resume-1\""));

    Ok(())
}

/// Validates that a Codex listen worker uses the continuation prompt and resume
/// argument when a stored thread handle is available for the next turn.
#[cfg(unix)]
#[test]
fn listen_worker_codex_uses_continuation_prompt_on_resumed_turn() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let home_dir = temp.path().join("home");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let api_url = server.url.clone();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&home_dir)?;
    fs::create_dir_all(&stub_dir)?;
    fs::create_dir_all(home_dir.join(".codex"))?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
"#,
        ),
    )?;
    fs::write(
        home_dir.join(".codex/config.toml"),
        r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
"#,
    )?;

    // Stub codex binary: tracks turns, captures prompt/instructions/args, outputs thread_id.
    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  cat <<'EOF'
-a, --ask-for-approval <APPROVAL_POLICY>
-s, --sandbox <SANDBOX_MODE>
-C, --cd <DIR>
    --add-dir <DIR>
    --dangerously-bypass-approvals-and-sandbox
EOF
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  cat <<'EOF'
-m, --model <MODEL>
-c, --config <key=value>
    --json
EOF
  exit 0
fi
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s\n' "$@" > "$TEST_OUTPUT_DIR/codex-args-$count.txt"
printf '%s' "$METASTACK_AGENT_PROMPT" > "$TEST_OUTPUT_DIR/prompt-$count.txt"
printf '%s' "$METASTACK_AGENT_INSTRUCTIONS" > "$TEST_OUTPUT_DIR/instructions-$count.txt"
mkdir -p src
printf '// turn %s\n' "$count" > "src/turn-$count.rs"
printf '%s\n' '{"type":"thread.started","thread_id":"codex-thread-resume-1"}'
printf '%s' '{"type":"item.completed","item":{"type":"agent_message","text":"codex listen ok"}}'
"#,
    )?;
    let mut permissions = fs::metadata(&codex_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&codex_path, permissions)?;

    init_repo_with_origin(&repo_root)?;
    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;

    // Start with a stored resume handle so the resumed-turn prompt path is exercised
    // deterministically in a single worker invocation.
    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "issue-32",
            "issue_identifier": "MET-32",
            "issue_title": "Session resume codex continuation test",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/MET-32",
            "phase": "blocked",
            "summary": "Waiting for Codex continuation turn",
            "brief_path": null,
            "workspace_path": workspace.display().to_string(),
            "workpad_comment_id": "comment-32",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": null,
            "latest_resume_handle": {
                "provider": "codex",
                "id": "codex-thread-resume-1"
            },
            "turns": 1,
            "tokens": {},
            "canonical": {},
            "log_path": "logs/MET-32.log"
        })],
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("HOME", &home_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "--workspace",
            workspace.to_str().expect("workspace path should be utf-8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--agent",
            "codex",
            "--max-turns",
            "2",
        ])
        .assert()
        .success();

    // Verify the resumed turn ran exactly once.
    let turn_count = fs::read_to_string(stub_dir.join("count.txt"))?
        .trim()
        .parse::<u32>()?;
    assert_eq!(turn_count, 1, "expected exactly one resumed agent turn");

    let prompt_1 = fs::read_to_string(stub_dir.join("prompt-1.txt"))?;
    let instructions_1 = fs::read_to_string(stub_dir.join("instructions-1.txt"))?;
    assert!(
        prompt_1.contains("Continuation guidance"),
        "resumed turn should receive continuation prompt, got: {}",
        &prompt_1[..prompt_1.len().min(200)]
    );
    assert!(
        !prompt_1.contains("You are working on Linear ticket"),
        "resumed turn should not include full issue context"
    );
    assert!(
        instructions_1.is_empty(),
        "resumed turn should have empty instructions on resume"
    );

    let args_1 = fs::read_to_string(stub_dir.join("codex-args-1.txt"))?;
    assert!(
        args_1.contains("resume"),
        "resumed turn should have resume arg"
    );
    assert!(
        args_1.contains("codex-thread-resume-1"),
        "resumed turn should reuse the stored thread id"
    );

    // Session state should have the new resume handle.
    let state = fs::read_to_string(state_path)?;
    assert!(state.contains("\"id\": \"codex-thread-resume-1\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_fails_closed_on_malformed_verifier_output_and_keeps_draft_artifacts()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let api_url = server.url.clone();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"],
    "repair_attempts": 0
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "exec-stub"

[agents.routing.commands."agents.listen.verification"]
provider = "verify-stub"

[agents.commands.exec-stub]
command = "exec-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.verify-stub]
command = "verify-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
battle_test_count = 0
"#,
        ),
    )?;
    fs::write(
        bin_dir.join("exec-stub"),
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/exec-count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$METASTACK_AGENT_PROMPT" > "$TEST_OUTPUT_DIR/exec-prompt-$count.txt"
mkdir -p src
printf '// malformed verification test\n' > src/exec-turn.rs
"#,
    )?;
    fs::write(
        bin_dir.join("verify-stub"),
        r#"#!/bin/sh
printf '%s' "$METASTACK_AGENT_PROMPT" > "$TEST_OUTPUT_DIR/verify-prompt.txt"
printf '%s' 'not-json'
"#,
    )?;
    for command_name in ["exec-stub", "verify-stub"] {
        let mut permissions = fs::metadata(bin_dir.join(command_name))?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(bin_dir.join(command_name), permissions)?;
    }
    write_listen_github_stub(
        &bin_dir.join("gh"),
        "ready",
        "https://github.com/example/repo/pull/321",
    )?;

    init_repo_with_origin(&repo_root)?;
    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    let recipe_dir = workspace.join(format!("{}/verification/recipes", branding::PROJECT_DIR));
    fs::create_dir_all(&recipe_dir)?;
    fs::write(
        recipe_dir.join("agents.listen.yaml"),
        r#"quality_criteria:
  - Verification proof.
"#,
    )?;
    write_listen_verification_report(
        &config_path,
        &repo_root,
        "MET-32",
        json!({
            "version": 1,
            "issue_identifier": "MET-32",
            "turn_number": 0,
            "generated_at_epoch_seconds": 1_773_575_000u64,
            "status": "failed",
            "summary": "Previous verification failed.",
            "route": {
                "route_key": "agents.listen.verification",
                "provider": "verify-stub",
                "provider_source": "command_route:agents.listen.verification"
            },
            "quality_criteria": [
                "Verification proof."
            ],
            "code_review": {
                "status": "failed",
                "summary": "Previous verification failed.",
                "criteria": [
                    {
                        "name": "Verification proof.",
                        "status": "failed",
                        "summary": "Verification needs repair.",
                        "findings": [],
                        "remediation": "Fix verification issue"
                    }
                ],
                "notes": []
            },
            "e2e": {
                "status": "skipped",
                "summary": "Not run.",
                "steps": []
            },
            "battle_tests": {
                "status": "skipped",
                "summary": "Not run.",
                "sampled_count": 0,
                "cases": []
            },
            "remediation": [
                "Fix verification issue"
            ],
            "notes": []
        }),
    )?;

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "issue-32",
            "issue_identifier": "MET-32",
            "issue_title": "Verification fail-closed",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/MET-32",
            "phase": "blocked",
            "summary": "Waiting for verification",
            "brief_path": null,
            "workspace_path": workspace.display().to_string(),
            "branch": "main",
            "pull_request": {
                "number": 321,
                "url": "https://github.com/example/repo/pull/321",
                "status": "draft"
            },
            "workpad_comment_id": "comment-32",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": null,
            "turns": 0,
            "tokens": {},
            "canonical": {},
            "log_path": "logs/MET-32.log"
        })],
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--max-turns",
            "1",
        ])
        .assert()
        .success();

    wait_for_file_substring(&state_path, "\"phase\": \"blocked\"")?;
    let state = fs::read_to_string(&state_path)?;
    assert!(state.contains("\"status\": \"draft\""));

    let verification_json = listen_verification_json_path(&config_path, &repo_root, "MET-32")?;
    let verification_markdown =
        listen_verification_markdown_path(&config_path, &repo_root, "MET-32")?;
    wait_for_file_substring(&verification_json, "\"status\": \"failed\"")?;
    let report = fs::read_to_string(&verification_json)?;
    assert!(report.contains("Verifier output was malformed"));
    assert!(report.contains("\"route_key\": \"agents.listen.verification\""));
    assert!(report.contains("\"provider\": \"verify-stub\""));

    let markdown = fs::read_to_string(&verification_markdown)?;
    assert!(markdown.contains("Status: Failed"));
    assert!(markdown.contains("agents.listen.verification"));

    let detail = fs::read_to_string(listen_detail_path(&config_path, &repo_root, "MET-32")?)?;
    assert!(detail.contains("\"verification\""));
    assert!(detail.contains("\"verification_json_path\""));
    assert!(detail.contains("\"verification_markdown_path\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_reenters_execution_with_verification_remediation_and_records_e2e_and_battle_tests()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let api_url = server.url.clone();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  },
  "validation": {
    "commands": ["true"],
    "repair_attempts": 1
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "exec-stub"

[agents.routing.commands."agents.listen.verification"]
provider = "verify-stub"

[agents.commands.exec-stub]
command = "exec-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.verify-stub]
command = "verify-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
battle_test_count = 1
"#,
        ),
    )?;
    fs::write(
        bin_dir.join("exec-stub"),
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/exec-count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$METASTACK_AGENT_PROMPT" > "$TEST_OUTPUT_DIR/exec-prompt-$count.txt"
mkdir -p src
printf '// execution turn %s\n' "$count" > "src/exec-turn-$count.rs"
"#,
    )?;
    fs::write(
        bin_dir.join("verify-stub"),
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/verify-count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$METASTACK_AGENT_PROMPT" > "$TEST_OUTPUT_DIR/verify-prompt-$count.txt"
if [ "$count" -eq 1 ]; then
  printf '%s' '{"summary":"Initial verification failed","criteria":[{"name":"Branch satisfies the verification proof.","status":"failed","summary":"Verification still needs a repair.","remediation":"Fix verification issue"}],"battle_tests":[{"input_path":".intuition/verification/inputs/agents.listen/sample.md","status":"failed","summary":"Sample input failed.","remediation":"Handle sample input"}],"notes":["first verification failed"]}'
else
  printf '%s' '{"summary":"Verification passed","criteria":[{"name":"Branch satisfies the verification proof.","status":"passed","summary":"Verification proof is satisfied."}],"battle_tests":[{"input_path":".intuition/verification/inputs/agents.listen/sample.md","status":"passed","summary":"Sample input passed."}],"notes":["second verification passed"]}'
fi
"#,
    )?;
    for command_name in ["exec-stub", "verify-stub"] {
        let mut permissions = fs::metadata(bin_dir.join(command_name))?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(bin_dir.join(command_name), permissions)?;
    }

    init_repo_with_origin(&repo_root)?;
    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "ready",
        "https://github.com/example/repo/pull/321",
    )?;

    let recipe_dir = workspace.join(format!("{}/verification/recipes", branding::PROJECT_DIR));
    fs::create_dir_all(&recipe_dir)?;
    fs::write(
        recipe_dir.join("agents.listen.yaml"),
        r#"quality_criteria:
  - Branch satisfies the verification proof.
e2e:
  - name: workspace-proof
    command:
      - sh
      - -c
      - printf verification-ok && touch .verification-e2e.txt
    expect_stdout_contains:
      - verification-ok
    expect_paths_exist:
      - .verification-e2e.txt
"#,
    )?;
    let input_dir = workspace.join(format!(
        "{}/verification/inputs/agents.listen",
        branding::PROJECT_DIR
    ));
    fs::create_dir_all(&input_dir)?;
    fs::write(input_dir.join("sample.md"), "sample battle input\n")?;
    write_listen_verification_report(
        &config_path,
        &repo_root,
        "MET-32",
        json!({
            "version": 1,
            "issue_identifier": "MET-32",
            "turn_number": 0,
            "generated_at_epoch_seconds": 1_773_575_000u64,
            "status": "failed",
            "summary": "Previous verification failed.",
            "route": {
                "route_key": "agents.listen.verification",
                "provider": "verify-stub",
                "provider_source": "repo_default"
            },
            "quality_criteria": [],
            "code_review": {
                "status": "failed",
                "summary": "Previous verification failed.",
                "criteria": [],
                "notes": []
            },
            "e2e": {
                "status": "skipped",
                "summary": "Not run.",
                "steps": []
            },
            "battle_tests": {
                "status": "skipped",
                "summary": "Not run.",
                "sampled_count": 0,
                "cases": []
            },
            "remediation": [
                "Repair the verification findings and rerun verification."
            ],
            "notes": []
        }),
    )?;

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "issue-32",
            "issue_identifier": "MET-32",
            "issue_title": "Verification retry loop",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/MET-32",
            "phase": "blocked",
            "summary": "Waiting for verification retry",
            "brief_path": null,
            "workspace_path": workspace.display().to_string(),
            "branch": "main",
            "pull_request": {
                "number": 321,
                "url": "https://github.com/example/repo/pull/321",
                "status": "draft"
            },
            "workpad_comment_id": "comment-32",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": null,
            "turns": 0,
            "tokens": {},
            "canonical": {},
            "log_path": "logs/MET-32.log"
        })],
    )?;
    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--max-turns",
            "2",
        ])
        .assert()
        .success();

    wait_for_path(&stub_dir.join("exec-prompt-2.txt"))?;
    wait_for_path(&stub_dir.join("verify-count.txt"))?;
    let verify_count = fs::read_to_string(stub_dir.join("verify-count.txt"))?;
    let log_contents = listen_log_path(&config_path, &repo_root, "MET-32")
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    let state_snapshot = fs::read_to_string(&state_path).unwrap_or_default();
    assert_eq!(
        verify_count.trim(),
        "2",
        "state={state_snapshot}\nlog={log_contents}"
    );

    let verification_json = listen_verification_json_path(&config_path, &repo_root, "MET-32")?;
    wait_for_path(&verification_json)?;
    wait_for_file_substring(&verification_json, "\"turn_number\": 2")?;
    let report_text = fs::read_to_string(&verification_json)?;
    let report: serde_json::Value = serde_json::from_str(&report_text)?;
    assert_eq!(report["status"], "passed", "report={report_text}");
    assert_eq!(report["e2e"]["status"], "passed");
    assert_eq!(report["battle_tests"]["status"], "passed");
    assert_eq!(report["battle_tests"]["sampled_count"], 1);
    assert_eq!(
        report["battle_tests"]["cases"][0]["input_path"],
        ".intuition/verification/inputs/agents.listen/sample.md"
    );
    assert!(
        report["e2e"]["steps"][0]["stdout_excerpt"]
            .as_str()
            .unwrap_or_default()
            .contains("verification-ok")
    );
    assert!(workspace.join(".verification-e2e.txt").is_file());

    let state = fs::read_to_string(&state_path)?;
    assert!(state.contains("\"phase\": \"completed\""));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_keeps_validation_repair_budget_after_verification_retry()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let api_url = server.url.clone();
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET"
  },
  "validation": {
    "commands": ["validation-stub"],
    "repair_attempts": 1
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "exec-stub"

[agents.routing.commands."agents.listen.verification"]
provider = "verify-stub"

[agents.commands.exec-stub]
command = "exec-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.verify-stub]
command = "verify-stub"
args = ["{{{{payload}}}}"]
transport = "arg"

[verification]
battle_test_count = 0
"#,
        ),
    )?;
    fs::write(
        bin_dir.join("exec-stub"),
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/exec-count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
mkdir -p src
printf '// execution turn %s\n' "$count" > "src/exec-turn-$count.rs"
"#,
    )?;
    fs::write(
        bin_dir.join("verify-stub"),
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/verify-count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
if [ "$count" -eq 1 ]; then
  printf '%s' '{"summary":"Initial verification failed","criteria":[{"name":"Verification proof.","status":"failed","summary":"Verification needs repair.","remediation":"Fix verification issue"}],"battle_tests":[],"notes":[]}'
else
  printf '%s' '{"summary":"Verification passed","criteria":[{"name":"Verification proof.","status":"passed","summary":"Verification proof is satisfied."}],"battle_tests":[],"notes":[]}'
fi
"#,
    )?;
    fs::write(
        bin_dir.join("validation-stub"),
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/validation-count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
if [ "$count" -eq 1 ]; then
  printf '%s\n' 'validation failed once' >&2
  exit 1
fi
printf '%s\n' 'validation passed'
"#,
    )?;
    for command_name in ["exec-stub", "verify-stub", "validation-stub"] {
        let mut permissions = fs::metadata(bin_dir.join(command_name))?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(bin_dir.join(command_name), permissions)?;
    }

    init_repo_with_origin(&repo_root)?;
    let workspace = create_workspace_clone_checkout(&repo_root, "repo-workspace/MET-32")?;
    write_listen_github_stub_for_workspace_head(
        &bin_dir.join("gh"),
        &workspace,
        "ready",
        "https://github.com/example/repo/pull/321",
    )?;
    let recipe_dir = workspace.join(format!("{}/verification/recipes", branding::PROJECT_DIR));
    fs::create_dir_all(&recipe_dir)?;
    fs::write(
        recipe_dir.join("agents.listen.yaml"),
        r#"quality_criteria:
  - Verification proof.
"#,
    )?;
    let backlog_dir = workspace.join(format!("{}/backlog/MET-32", branding::PROJECT_DIR));
    fs::create_dir_all(&backlog_dir)?;
    fs::write(
        backlog_dir.join("index.md"),
        "# MET-32\n\n## Tasks\n\n- [x] Verification ready\n",
    )?;

    let state_path = write_listen_store_session(
        &config_path,
        &repo_root,
        vec![json!({
            "issue_id": "issue-32",
            "issue_identifier": "MET-32",
            "issue_title": "Split verification and validation repair budgets",
            "project_name": "MetaStack CLI",
            "team_key": "MET",
            "issue_url": "https://linear.app/issues/MET-32",
            "phase": "blocked",
            "summary": "Waiting for verification retry",
            "brief_path": null,
            "workspace_path": workspace.display().to_string(),
            "branch": "main",
            "pull_request": {
                "number": 321,
                "url": "https://github.com/example/repo/pull/321",
                "status": "draft"
            },
            "workpad_comment_id": "comment-32",
            "updated_at_epoch_seconds": 1_773_575_100u64,
            "pid": null,
            "session_id": null,
            "turns": 0,
            "tokens": {},
            "canonical": {},
            "log_path": "logs/MET-32.log"
        })],
    )?;

    let current_path = std::env::var("PATH")?;
    meta()
        .current_dir(&workspace)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env("PATH", format!("{}:{}", bin_dir.display(), current_path))
        .args([
            "listen-worker",
            "--source-root",
            repo_root.to_str().expect("utf8"),
            "--workspace",
            workspace.to_str().expect("utf8"),
            "--issue",
            "MET-32",
            "--workpad-comment-id",
            "comment-32",
            "--backlog-issue",
            "MET-32",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--max-turns",
            "3",
        ])
        .assert()
        .success();

    wait_for_path(&stub_dir.join("validation-count.txt"))?;
    assert_eq!(
        fs::read_to_string(stub_dir.join("validation-count.txt"))?.trim(),
        "2"
    );

    let state = fs::read_to_string(&state_path)?;
    assert!(state.contains("\"phase\": \"completed\""), "state={state}");

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_reports_green_post_publication_ci_in_inspect_and_completion_summary()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let planning_context = r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#;
    let (repo_root, config_path, bin_dir, stub_dir, workspace) =
        prepare_post_publication_ci_fixture(
            temp.path(),
            server.url.as_str(),
            planning_context,
            r#"
[defaults.listen]
ci_poll_interval_seconds = 1
ci_poll_timeout_seconds = 2
ci_timeout_behavior = "block"
"#,
            "#!/bin/sh\n:\n",
            "all-pass",
            "met-32-ci-all-pass",
        )?;

    run_listen_worker_fixture(
        &repo_root,
        &config_path,
        &bin_dir,
        &stub_dir,
        &workspace,
        server.url.as_str(),
        1,
    )?;

    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(state.contains("\"phase\": \"completed\""));
    assert!(state.contains("GitHub CI passed 1/1"));

    let inspect = inspect_listen_sessions(&repo_root, &config_path)?;
    assert!(inspect.contains("GitHub CI passed 1/1"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_waits_for_pending_checks_then_passes_and_surfaces_waiting_progress()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let planning_context = r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#;
    let (repo_root, config_path, bin_dir, stub_dir, workspace) =
        prepare_post_publication_ci_fixture(
            temp.path(),
            server.url.as_str(),
            planning_context,
            r#"
[defaults.listen]
ci_poll_interval_seconds = 1
ci_poll_timeout_seconds = 3
ci_timeout_behavior = "block"
"#,
            "#!/bin/sh\n:\n",
            "pending-then-pass",
            "met-32-ci-pending-pass",
        )?;

    run_listen_worker_fixture(
        &repo_root,
        &config_path,
        &bin_dir,
        &stub_dir,
        &workspace,
        server.url.as_str(),
        1,
    )?;

    assert_eq!(
        fs::read_to_string(stub_dir.join("gh-checks-count.txt"))?.trim(),
        "2"
    );

    let inspect = inspect_listen_sessions(&repo_root, &config_path)?;
    assert!(inspect.contains("waiting for GitHub CI 0/1 settled"));
    assert!(inspect.contains("GitHub CI passed 1/1"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_waits_for_pending_checks_then_repairs_the_same_pull_request()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let planning_context = r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["sh -lc 'count_file=\"$TEST_OUTPUT_DIR/validation-count.txt\"; count=0; [ -f \"$count_file\" ] && count=$(cat \"$count_file\"); count=$((count + 1)); printf \"%s\" \"$count\" > \"$count_file\"; test -f repaired.txt'"],
    "repair_attempts": 2,
    "profile": "ci-repair"
  }
}
"#;
    let agent_script = r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
printf '%s' "$1" > "$TEST_OUTPUT_DIR/payload-$count.txt"
printf '%s' 'ok' > repaired.txt
"#;
    let (repo_root, config_path, bin_dir, stub_dir, workspace) =
        prepare_post_publication_ci_fixture(
            temp.path(),
            server.url.as_str(),
            planning_context,
            r#"
[defaults.listen]
ci_poll_interval_seconds = 1
ci_poll_timeout_seconds = 3
ci_timeout_behavior = "block"
"#,
            agent_script,
            "pending-then-fail",
            "met-32-ci-pending-fail",
        )?;

    run_listen_worker_fixture(
        &repo_root,
        &config_path,
        &bin_dir,
        &stub_dir,
        &workspace,
        server.url.as_str(),
        2,
    )?;

    assert_eq!(fs::read_to_string(stub_dir.join("count.txt"))?.trim(), "2");
    assert_eq!(
        fs::read_to_string(stub_dir.join("validation-count.txt"))?.trim(),
        "2"
    );
    assert!(
        fs::read_to_string(stub_dir.join("payload-2.txt"))?
            .contains("Repair failing GitHub checks on PR #321 and update the same PR."),
    );
    let gh_log = fs::read_to_string(stub_dir.join("gh.log"))?;
    assert_eq!(
        gh_log
            .matches("pr create --base main --head met-32-ci-pending-fail")
            .count(),
        1
    );
    assert!(gh_log.contains("pr edit 321 --title MET-32: Continuation loop --body-file"));
    assert_eq!(
        fs::read_to_string(stub_dir.join("gh-checks-count.txt"))?.trim(),
        "3"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_blocks_when_post_publication_ci_times_out() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let planning_context = r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#;
    let (repo_root, config_path, bin_dir, stub_dir, workspace) =
        prepare_post_publication_ci_fixture(
            temp.path(),
            server.url.as_str(),
            planning_context,
            r#"
[defaults.listen]
ci_poll_interval_seconds = 1
ci_poll_timeout_seconds = 1
ci_timeout_behavior = "block"
"#,
            "#!/bin/sh\n:\n",
            "pending-always",
            "met-32-ci-timeout-block",
        )?;

    run_listen_worker_fixture(
        &repo_root,
        &config_path,
        &bin_dir,
        &stub_dir,
        &workspace,
        server.url.as_str(),
        1,
    )?;

    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(state.contains("\"phase\": \"blocked\""));
    assert!(state.contains("GitHub CI settle timed out after 1s"));
    assert_eq!(
        fs::read_to_string(stub_dir.join("gh-checks-count.txt"))?.trim(),
        "2"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_worker_warns_and_proceeds_when_post_publication_ci_times_out()
-> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let planning_context = r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#;
    let (repo_root, config_path, bin_dir, stub_dir, workspace) =
        prepare_post_publication_ci_fixture(
            temp.path(),
            server.url.as_str(),
            planning_context,
            r#"
[defaults.listen]
ci_poll_interval_seconds = 1
ci_poll_timeout_seconds = 1
ci_timeout_behavior = "warn_and_proceed"
"#,
            "#!/bin/sh\n:\n",
            "pending-always",
            "met-32-ci-timeout-warn",
        )?;

    run_listen_worker_fixture(
        &repo_root,
        &config_path,
        &bin_dir,
        &stub_dir,
        &workspace,
        server.url.as_str(),
        1,
    )?;

    let state = fs::read_to_string(listen_state_path(&config_path, &repo_root)?)?;
    assert!(state.contains("\"phase\": \"completed\""));
    assert!(state.contains("GitHub CI timeout warning after 1s"));

    let inspect = inspect_listen_sessions(&repo_root, &config_path)?;
    assert!(inspect.contains("GitHub CI timeout warning after 1s"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn listen_surfaces_no_checks_configured_in_inspect_and_dashboard() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let server = DynamicLinearServer::start_with_completion_after_refreshes(1_000_000)?;
    let planning_context = r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "validation": {
    "commands": ["true"]
  }
}
"#;
    let (repo_root, config_path, bin_dir, stub_dir, workspace) =
        prepare_post_publication_ci_fixture(
            temp.path(),
            server.url.as_str(),
            planning_context,
            r#"
[defaults.listen]
ci_poll_interval_seconds = 1
ci_poll_timeout_seconds = 2
ci_timeout_behavior = "block"
"#,
            "#!/bin/sh\n:\n",
            "pass",
            "met-32-ci-no-checks",
        )?;

    run_listen_worker_fixture(
        &repo_root,
        &config_path,
        &bin_dir,
        &stub_dir,
        &workspace,
        server.url.as_str(),
        1,
    )?;

    let state_path = listen_state_path(&config_path, &repo_root)?;
    wait_for_file_substring(&state_path, "no GitHub checks configured")?;
    let inspect = inspect_listen_sessions(&repo_root, &config_path)?;
    assert!(inspect.contains("no GitHub checks configured"));

    Ok(())
}
