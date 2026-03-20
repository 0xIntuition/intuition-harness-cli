#![allow(dead_code, unused_imports)]

include!("support/common.rs");

#[cfg(unix)]
fn write_onboarded_config(
    config_path: &Path,
    config: impl AsRef<str>,
) -> Result<(), Box<dyn Error>> {
    let contents = format!(
        "{}\n[onboarding]\ncompleted = true\n",
        config.as_ref().trim_end()
    );
    fs::write(config_path, &contents)?;
    let home_config = isolated_home_dir().join(".config/metastack/config.toml");
    if let Some(parent) = home_config.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(home_config, contents)?;
    Ok(())
}

#[cfg(unix)]
fn prepend_path(bin_dir: &Path) -> Result<String, Box<dyn Error>> {
    let current_path = std::env::var("PATH")?;
    Ok(format!("{}:{}", bin_dir.display(), current_path))
}

#[cfg(unix)]
fn configure_github_remote_alias(
    repo_root: &Path,
    git_config: &Path,
) -> Result<(), Box<dyn Error>> {
    let bare = repo_root
        .parent()
        .expect("repo should have a parent")
        .join("origin.git");
    fs::write(
        git_config,
        format!(
            "[url \"{}\"]\n\tinsteadOf = git@github.com:metastack-systems/metastack-cli.git\n",
            bare.display()
        ),
    )?;

    let status = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "remote",
            "set-url",
            "origin",
            "git@github.com:metastack-systems/metastack-cli.git",
        ])
        .status()?;
    assert!(status.success());
    Ok(())
}

#[cfg(unix)]
fn create_pr_head_branch(
    repo_root: &Path,
    branch: &str,
    git_config: &Path,
) -> Result<(), Box<dyn Error>> {
    fs::write(repo_root.join("src.txt"), "base\n")?;
    let status = ProcessCommand::new("git")
        .env("GIT_CONFIG_GLOBAL", git_config)
        .arg("-C")
        .arg(repo_root)
        .args(["add", "src.txt"])
        .status()?;
    assert!(status.success());
    let status = ProcessCommand::new("git")
        .env("GIT_CONFIG_GLOBAL", git_config)
        .arg("-C")
        .arg(repo_root)
        .args(["commit", "-m", "Seed repo"])
        .status()?;
    assert!(status.success());
    let status = ProcessCommand::new("git")
        .env("GIT_CONFIG_GLOBAL", git_config)
        .arg("-C")
        .arg(repo_root)
        .args(["push", "origin", "main"])
        .status()?;
    assert!(status.success());

    let status = ProcessCommand::new("git")
        .env("GIT_CONFIG_GLOBAL", git_config)
        .arg("-C")
        .arg(repo_root)
        .args(["checkout", "-B", branch])
        .status()?;
    assert!(status.success());
    fs::write(
        repo_root.join("feature.txt"),
        format!("feature branch: {branch}\n"),
    )?;
    let status = ProcessCommand::new("git")
        .env("GIT_CONFIG_GLOBAL", git_config)
        .arg("-C")
        .arg(repo_root)
        .args(["add", "feature.txt"])
        .status()?;
    assert!(status.success());
    let status = ProcessCommand::new("git")
        .env("GIT_CONFIG_GLOBAL", git_config)
        .arg("-C")
        .arg(repo_root)
        .args(["commit", "-m", "Feature branch"])
        .status()?;
    assert!(status.success());
    let status = ProcessCommand::new("git")
        .env("GIT_CONFIG_GLOBAL", git_config)
        .arg("-C")
        .arg(repo_root)
        .args(["push", "-u", "origin", branch])
        .status()?;
    assert!(status.success());
    let status = ProcessCommand::new("git")
        .env("GIT_CONFIG_GLOBAL", git_config)
        .arg("-C")
        .arg(repo_root)
        .args(["checkout", "main"])
        .status()?;
    assert!(status.success());
    Ok(())
}

#[cfg(unix)]
fn write_review_agent_stub(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
mode="${REVIEW_AGENT_MODE:-no-remediation}"
prompt="${METASTACK_AGENT_PROMPT:-}"
if printf '%s' "$prompt" | grep -q "Return exactly one JSON object"; then
  if [ "$mode" = "remediation" ]; then
    printf '%s' '{"remediation_required":true,"summary":"Blocking issue found in the PR.","required_fixes":[{"title":"Fix generated artifact","rationale":"The change needs a remediation commit before the PR is safe.","file_hints":["REVIEW_FIX.txt"]}],"optional_recommendations":[{"title":"Document the follow-up","rationale":"Optional docs note for reviewers.","file_hints":[]}]}'
  else
    printf '%s' '{"remediation_required":false,"summary":"No blocking issues found.","required_fixes":[],"optional_recommendations":[{"title":"Optional docs note","rationale":"Non-blocking improvement.","file_hints":[]}]}'
  fi
  exit 0
fi
printf '%s\n' 'remediated by stub agent' > REVIEW_FIX.txt
printf '%s' 'applied remediation'
"#,
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn write_review_gh_stub(
    path: &Path,
    pull_requests_json: &str,
    detail_json: &str,
    diff: &str,
    remediation_url: &str,
) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        format!(
            r#"#!/bin/sh
set -eu
if [ "$1" = "-R" ]; then
  shift 2
fi
if [ "$1" = "repo" ] && [ "$2" = "view" ]; then
  printf '%s' '{{"nameWithOwner":"metastack-systems/metastack-cli","url":"https://github.com/metastack-systems/metastack-cli","defaultBranchRef":{{"name":"main"}}}}'
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  case " $* " in
    *" --head meta-review/"*)
      printf '%s' '[]'
      ;;
    *)
      printf '%s' '{pull_requests_json}'
      ;;
  esac
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  printf '%s' '{detail_json}'
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "diff" ]; then
  printf '%s' '{diff}'
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "create" ]; then
  printf '%s' '{{"url":"{remediation_url}"}}'
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
fn write_review_planning_context(
    repo_root: &Path,
    _api_url: &str,
    _agent_command: &Path,
) -> Result<(), Box<dyn Error>> {
    write_minimal_planning_context(
        repo_root,
        r#"{
  "linear": {
    "team": "ENG"
  }
}
"#,
    )?;
    fs::write(
        repo_root.join(".metastack/codebase/CONVENTIONS.md"),
        "Use focused fixes.\n",
    )?;
    fs::write(
        repo_root.join(".metastack/codebase/SCAN.md"),
        "Scan context.\n",
    )?;
    Ok(())
}

#[cfg(unix)]
fn review_agent_config(api_url: &str, agent_command: &Path) -> String {
    format!(
        r#"[linear]
api_url = "{api_url}"
api_key = "token"

[agents]
default_agent = "stub"

[agents.commands.stub]
command = "{}"
"#,
        agent_command.display()
    )
}

#[cfg(unix)]
fn write_linear_review_mocks(
    server: &MockServer,
    _expect_comment_create: bool,
) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-1",
                        "identifier": "ENG-10256",
                        "title": "Implement unified review",
                        "description": "# Title\n\n## Acceptance Criteria\n- Run review\n- Open remediation only when required\n",
                        "url": "https://linear.app/issues/ENG-10256",
                        "priority": 2,
                        "estimate": 3.0,
                        "updatedAt": "2026-03-20T10:00:00Z",
                        "team": {"id": "team-1", "key": "ENG", "name": "Engineering"},
                        "project": {"id": "project-1", "name": "MetaStack CLI"},
                        "assignee": null,
                        "labels": {"nodes": []},
                        "state": {"id": "state-1", "name": "In Progress", "type": "started"},
                        "attachments": {"nodes": []}
                    }],
                    "pageInfo": {"hasNextPage": false, "endCursor": null}
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue");
        then.status(200).json_body(json!({
            "data": {
                "issue": {
                    "id": "issue-1",
                    "identifier": "ENG-10256",
                    "title": "Implement unified review",
                    "description": "# Title\n\n## Acceptance Criteria\n- Run review\n- Open remediation only when required\n",
                    "url": "https://linear.app/issues/ENG-10256",
                    "priority": 2,
                    "estimate": 3.0,
                    "updatedAt": "2026-03-20T10:00:00Z",
                    "team": {"id": "team-1", "key": "ENG", "name": "Engineering"},
                    "project": {"id": "project-1", "name": "MetaStack CLI"},
                    "assignee": null,
                    "labels": {"nodes": []},
                    "comments": {
                        "nodes": [{
                            "id": "comment-1",
                            "body": "## Codex Workpad\n\nActive notes",
                            "createdAt": "2026-03-20T09:00:00Z",
                            "user": {"name": "Reviewer"},
                            "resolvedAt": null
                        }],
                        "pageInfo": {"hasNextPage": false, "endCursor": null}
                    },
                    "state": {"id": "state-1", "name": "In Progress", "type": "started"},
                    "attachments": {"nodes": []},
                    "parent": null,
                    "children": {"nodes": []}
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-created",
                        "body": "created",
                        "resolvedAt": null
                    }
                }
            }
        }));
    })
}

#[cfg(unix)]
fn review_store_dir(config_path: &Path, repo_root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    repo_root
        .canonicalize()?
        .display()
        .to_string()
        .hash(&mut hasher);
    "metastack-systems/metastack-cli".hash(&mut hasher);
    let key = format!("{:016x}", hasher.finish());
    Ok(config_path
        .parent()
        .expect("config path should have a parent")
        .join("data")
        .join("review")
        .join("projects")
        .join(key))
}

#[cfg(unix)]
#[test]
fn review_direct_dry_run_reports_planned_execution_and_diagnostics() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let git_config = temp.path().join("gitconfig");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::write(repo_root.join("README.md"), "seed\n")?;
    init_repo_with_origin(&repo_root)?;
    configure_github_remote_alias(&repo_root, &git_config)?;
    create_pr_head_branch(&repo_root, "feature/eng-10256", &git_config)?;

    let linear = MockServer::start();
    let agent_path = bin_dir.join("review-agent");
    write_review_agent_stub(&agent_path)?;
    write_onboarded_config(
        &config_path,
        review_agent_config(&linear.url("/graphql"), &agent_path),
    )?;
    write_review_planning_context(&repo_root, &linear.url("/graphql"), &agent_path)?;
    let _comment_mock = write_linear_review_mocks(&linear, false);

    write_review_gh_stub(
        &bin_dir.join("gh"),
        r#"[{"number":123,"title":"ENG-10256 add review","url":"https://github.com/metastack-systems/metastack-cli/pull/123","headRefName":"feature/eng-10256","baseRefName":"main","updatedAt":"2026-03-20T10:00:00Z","author":{"login":"kames"},"labels":[{"name":"metastack"}]}]"#,
        r#"{"number":123,"title":"ENG-10256 add review","body":"Implements ENG-10256","url":"https://github.com/metastack-systems/metastack-cli/pull/123","headRefName":"feature/eng-10256","baseRefName":"main","reviewDecision":"REVIEW_REQUIRED","changedFiles":1,"files":[{"path":"src/lib.rs","additions":10,"deletions":2}],"reviews":[],"author":{"login":"kames"}}"#,
        "diff --git a/src/lib.rs b/src/lib.rs",
        "https://github.com/metastack-systems/metastack-cli/pull/999",
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("GIT_CONFIG_GLOBAL", &git_config)
        .env("PATH", prepend_path(&bin_dir)?)
        .args([
            "agents",
            "review",
            "123",
            "--root",
            repo_root.to_str().expect("utf-8 path"),
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Dry run: no GitHub or Linear mutations were applied.",
        ))
        .stdout(predicate::str::contains(
            "Resolved route key: agents.review",
        ))
        .stdout(predicate::str::contains("ENG-10256"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn review_direct_no_remediation_exits_without_follow_up_pr() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let git_config = temp.path().join("gitconfig");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::write(repo_root.join("README.md"), "seed\n")?;
    init_repo_with_origin(&repo_root)?;
    configure_github_remote_alias(&repo_root, &git_config)?;
    create_pr_head_branch(&repo_root, "feature/eng-10256", &git_config)?;

    let linear = MockServer::start();
    let agent_path = bin_dir.join("review-agent");
    write_review_agent_stub(&agent_path)?;
    write_onboarded_config(
        &config_path,
        review_agent_config(&linear.url("/graphql"), &agent_path),
    )?;
    write_review_planning_context(&repo_root, &linear.url("/graphql"), &agent_path)?;
    let comment_mock = write_linear_review_mocks(&linear, false);

    write_review_gh_stub(
        &bin_dir.join("gh"),
        r#"[{"number":123,"title":"ENG-10256 add review","url":"https://github.com/metastack-systems/metastack-cli/pull/123","headRefName":"feature/eng-10256","baseRefName":"main","updatedAt":"2026-03-20T10:00:00Z","author":{"login":"kames"},"labels":[{"name":"metastack"}]}]"#,
        r#"{"number":123,"title":"ENG-10256 add review","body":"Implements ENG-10256","url":"https://github.com/metastack-systems/metastack-cli/pull/123","headRefName":"feature/eng-10256","baseRefName":"main","reviewDecision":"REVIEW_REQUIRED","changedFiles":1,"files":[{"path":"src/lib.rs","additions":10,"deletions":2}],"reviews":[],"author":{"login":"kames"}}"#,
        "diff --git a/src/lib.rs b/src/lib.rs",
        "https://github.com/metastack-systems/metastack-cli/pull/999",
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("GIT_CONFIG_GLOBAL", &git_config)
        .env("PATH", prepend_path(&bin_dir)?)
        .args([
            "agents",
            "review",
            "123",
            "--root",
            repo_root.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Remediation required: no"))
        .stdout(predicate::str::contains("Optional recommendations:"));

    comment_mock.assert_calls(0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn review_direct_remediation_creates_follow_up_pr_and_linear_comment() -> Result<(), Box<dyn Error>>
{
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let git_config = temp.path().join("gitconfig");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::write(repo_root.join("README.md"), "seed\n")?;
    init_repo_with_origin(&repo_root)?;
    configure_github_remote_alias(&repo_root, &git_config)?;
    create_pr_head_branch(&repo_root, "feature/eng-10256", &git_config)?;

    let linear = MockServer::start();
    let agent_path = bin_dir.join("review-agent");
    write_review_agent_stub(&agent_path)?;
    write_onboarded_config(
        &config_path,
        review_agent_config(&linear.url("/graphql"), &agent_path),
    )?;
    write_review_planning_context(&repo_root, &linear.url("/graphql"), &agent_path)?;
    let comment_mock = write_linear_review_mocks(&linear, true);

    write_review_gh_stub(
        &bin_dir.join("gh"),
        r#"[{"number":123,"title":"ENG-10256 add review","url":"https://github.com/metastack-systems/metastack-cli/pull/123","headRefName":"feature/eng-10256","baseRefName":"main","updatedAt":"2026-03-20T10:00:00Z","author":{"login":"kames"},"labels":[{"name":"metastack"}]}]"#,
        r#"{"number":123,"title":"ENG-10256 add review","body":"Implements ENG-10256","url":"https://github.com/metastack-systems/metastack-cli/pull/123","headRefName":"feature/eng-10256","baseRefName":"main","reviewDecision":"REVIEW_REQUIRED","changedFiles":1,"files":[{"path":"src/lib.rs","additions":10,"deletions":2}],"reviews":[],"author":{"login":"kames"}}"#,
        "diff --git a/src/lib.rs b/src/lib.rs",
        "https://github.com/metastack-systems/metastack-cli/pull/999",
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("GIT_CONFIG_GLOBAL", &git_config)
        .env("PATH", prepend_path(&bin_dir)?)
        .env("REVIEW_AGENT_MODE", "remediation")
        .args([
            "agents",
            "review",
            "123",
            "--root",
            repo_root.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Remediation required: yes"))
        .stdout(predicate::str::contains(
            "https://github.com/metastack-systems/metastack-cli/pull/999",
        ));

    comment_mock.assert_calls(1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn review_listener_json_and_lock_behave_deterministically() -> Result<(), Box<dyn Error>> {
    let _guard = listen_test_lock();
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let git_config = temp.path().join("gitconfig");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&bin_dir)?;
    fs::write(repo_root.join("README.md"), "seed\n")?;
    init_repo_with_origin(&repo_root)?;
    configure_github_remote_alias(&repo_root, &git_config)?;

    let linear = MockServer::start();
    let agent_path = bin_dir.join("review-agent");
    write_review_agent_stub(&agent_path)?;
    write_onboarded_config(
        &config_path,
        review_agent_config(&linear.url("/graphql"), &agent_path),
    )?;
    write_review_planning_context(&repo_root, &linear.url("/graphql"), &agent_path)?;
    let _comment_mock = write_linear_review_mocks(&linear, false);

    write_review_gh_stub(
        &bin_dir.join("gh"),
        r#"[{"number":123,"title":"ENG-10256 add review","url":"https://github.com/metastack-systems/metastack-cli/pull/123","headRefName":"feature/eng-10256","baseRefName":"main","updatedAt":"2026-03-20T10:00:00Z","author":{"login":"kames"},"labels":[{"name":"metastack"}]}]"#,
        r#"{"number":123,"title":"ENG-10256 add review","body":"Implements ENG-10256","url":"https://github.com/metastack-systems/metastack-cli/pull/123","headRefName":"feature/eng-10256","baseRefName":"main","reviewDecision":"REVIEW_REQUIRED","changedFiles":1,"files":[{"path":"src/lib.rs","additions":10,"deletions":2}],"reviews":[],"author":{"login":"kames"}}"#,
        "diff --git a/src/lib.rs b/src/lib.rs",
        "https://github.com/metastack-systems/metastack-cli/pull/999",
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("GIT_CONFIG_GLOBAL", &git_config)
        .env("PATH", prepend_path(&bin_dir)?)
        .args([
            "agents",
            "review",
            "--root",
            repo_root.to_str().expect("utf-8 path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"number\": 123"))
        .stdout(predicate::str::contains(
            "\"name_with_owner\": \"metastack-systems/metastack-cli\"",
        ));

    let store_dir = review_store_dir(&config_path, &repo_root)?;
    fs::create_dir_all(&store_dir)?;
    fs::write(
        store_dir.join("active-review.lock.json"),
        serde_json::to_vec_pretty(&json!({
            "pid": std::process::id(),
            "acquired_at_epoch_seconds": 1
        }))?,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("GIT_CONFIG_GLOBAL", &git_config)
        .env("PATH", prepend_path(&bin_dir)?)
        .args([
            "agents",
            "review",
            "--root",
            repo_root.to_str().expect("utf-8 path"),
            "--once",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "another `meta agents review` listener already owns this repository",
        ));

    Ok(())
}
