#![allow(dead_code, unused_imports)]

include!("support/common.rs");

#[cfg(unix)]
#[test]
fn agents_review_help_lists_the_subcommand() -> Result<(), Box<dyn Error>> {
    meta()
        .args(["agents", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("review"))
        .stdout(predicate::str::contains("pull request"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_review_dry_run_uses_route_resolution_and_renders_context() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let agent_stub = temp.path().join("review-agent");
    let gh_stub = temp.path().join("gh");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "ENG",
    "project_id": "project-1"
  }
}
"#,
    )?;
    fs::create_dir_all(repo_root.join(".metastack/backlog/ENG-10251/context"))?;
    fs::write(
        repo_root.join(".metastack/backlog/ENG-10251/index.md"),
        "# Local backlog\n",
    )?;
    fs::write(
        repo_root.join(".metastack/backlog/ENG-10251/context/ticket-discussion.md"),
        "# Discussion\n",
    )?;
    write_review_agent_config(&config_path, &api_url, &agent_stub, true)?;
    write_executable(&agent_stub, "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{}'\n")?;
    write_review_gh_stub(
        &gh_stub,
        "ENG-10251",
        false,
        "https://github.com/metastack-systems/metastack-cli/pull/12345",
    )?;

    let issue = review_issue_node(
        "issue-1",
        "ENG-10251",
        "Add review command",
        "# Description\n\nShip it.\n\n## Acceptance Criteria\n\n- Dry runs render diagnostics\n- Remediation PRs are GitHub-only for now\n",
        "state-2",
        "In Progress",
    );
    mock_issue_lookup(&server, vec![issue.clone()]);
    mock_review_issue_detail(
        &server,
        "issue-1",
        review_issue_detail_node(
            "issue-1",
            "ENG-10251",
            "Add review command",
            "# Description\n\nShip it.\n\n## Acceptance Criteria\n\n- Dry runs render diagnostics\n- Remediation PRs are GitHub-only for now\n",
            vec![json!({
                "id": "comment-1",
                "body": "## Codex Workpad\n\n- [ ] keep this current",
                "createdAt": "2026-03-20T12:00:00Z",
                "resolvedAt": null,
                "user": {"id":"user-1","name":"Codex","email":"codex@example.com"}
            })],
        ),
    );

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env(
            "PATH",
            format!("{}:{}", temp.path().display(), std::env::var("PATH")?),
        )
        .args([
            "agents",
            "review",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--dry-run",
            "12345",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Resolved route key: agents.review",
        ))
        .stdout(predicate::str::contains(
            "Resolved provider: route-review-stub",
        ))
        .stdout(predicate::str::contains("Linked Linear issue: `ENG-10251`"))
        .stdout(predicate::str::contains("Acceptance Criteria"))
        .stdout(predicate::str::contains("Repo context warnings: none"))
        .stdout(predicate::str::contains("## Active Workpad"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_review_fails_when_linear_linkage_is_missing() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let agent_stub = temp.path().join("review-agent");
    let gh_stub = temp.path().join("gh");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    write_minimal_planning_context(
        &repo_root,
        r#"{"linear":{"team":"ENG","project_id":"project-1"}}"#,
    )?;
    write_review_agent_config(&config_path, &api_url, &agent_stub, false)?;
    write_executable(&agent_stub, "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{}'\n")?;
    write_review_gh_stub(
        &gh_stub,
        "no-ticket-here",
        false,
        "https://github.com/metastack-systems/metastack-cli/pull/12345",
    )?;
    mock_issue_lookup(&server, vec![]);

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env(
            "PATH",
            format!("{}:{}", temp.path().display(), std::env::var("PATH")?),
        )
        .args([
            "agents",
            "review",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--dry-run",
            "12345",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "does not contain a recognizable Linear issue identifier",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_review_fails_when_linear_linkage_is_ambiguous() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let agent_stub = temp.path().join("review-agent");
    let gh_stub = temp.path().join("gh");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    write_minimal_planning_context(
        &repo_root,
        r#"{"linear":{"team":"ENG","project_id":"project-1"}}"#,
    )?;
    write_review_agent_config(&config_path, &api_url, &agent_stub, false)?;
    write_executable(&agent_stub, "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{}'\n")?;
    write_review_gh_stub(
        &gh_stub,
        "ENG-10251 and ENG-10252",
        false,
        "https://github.com/metastack-systems/metastack-cli/pull/12345",
    )?;
    mock_issue_lookup(
        &server,
        vec![
            review_issue_node(
                "issue-1",
                "ENG-10251",
                "First linked issue",
                "# Acceptance Criteria\n\n- First issue\n",
                "state-2",
                "In Progress",
            ),
            review_issue_node(
                "issue-2",
                "ENG-10252",
                "Second linked issue",
                "# Acceptance Criteria\n\n- Second issue\n",
                "state-2",
                "In Progress",
            ),
        ],
    );
    mock_review_issue_detail(
        &server,
        "issue-1",
        review_issue_detail_node(
            "issue-1",
            "ENG-10251",
            "First linked issue",
            "# Acceptance Criteria\n\n- First issue\n",
            Vec::new(),
        ),
    );
    mock_review_issue_detail(
        &server,
        "issue-2",
        review_issue_detail_node(
            "issue-2",
            "ENG-10252",
            "Second linked issue",
            "# Acceptance Criteria\n\n- Second issue\n",
            Vec::new(),
        ),
    );

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env(
            "PATH",
            format!("{}:{}", temp.path().display(), std::env::var("PATH")?),
        )
        .args([
            "agents",
            "review",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--dry-run",
            "12345",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "resolved to multiple Linear issues (ENG-10251, ENG-10252)",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_review_rejects_zero_pull_request_numbers() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");

    fs::create_dir_all(&repo_root)?;
    fs::write(
        &config_path,
        r#"[onboarding]
completed = true
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args(["agents", "review", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "pull request number must be greater than zero",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_review_no_fix_path_does_not_create_follow_up_pr() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let agent_stub = temp.path().join("review-agent");
    let gh_stub = temp.path().join("gh");
    let output_dir = temp.path().join("output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    write_minimal_planning_context(
        &repo_root,
        r#"{"linear":{"team":"ENG","project_id":"project-1"}}"#,
    )?;
    write_review_agent_config(&config_path, &api_url, &agent_stub, false)?;
    write_executable(
        &agent_stub,
        r#"#!/bin/sh
cat > "$TEST_OUTPUT_DIR/audit-prompt.txt"
printf '%s' '{"summary":"The PR satisfies the linked ticket.","requires_follow_up_changes":false,"blocking_issues":[],"optional_improvements":[{"title":"Clarify output wording","rationale":"The dry-run heading could be shorter.","evidence":["Current heading is verbose"],"suggested_fix":"Trim the dry-run heading."}],"broader_impact_areas":[],"suggested_validation":["cargo test --test agents_review"],"remediation_summary":null}'
"#,
    )?;
    write_review_gh_stub(
        &gh_stub,
        "ENG-10251",
        false,
        "https://github.com/metastack-systems/metastack-cli/pull/12345",
    )?;

    let issue = review_issue_node(
        "issue-1",
        "ENG-10251",
        "Add review command",
        "# Acceptance Criteria\n\n- The audit can decide that no changes are needed\n",
        "state-2",
        "In Progress",
    );
    mock_issue_lookup(&server, vec![issue.clone()]);
    mock_review_issue_detail(
        &server,
        "issue-1",
        review_issue_detail_node(
            "issue-1",
            "ENG-10251",
            "Add review command",
            "# Acceptance Criteria\n\n- The audit can decide that no changes are needed\n",
            Vec::new(),
        ),
    );

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .env(
            "PATH",
            format!("{}:{}", temp.path().display(), std::env::var("PATH")?),
        )
        .args([
            "agents",
            "review",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "12345",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Required follow-up changes: no"))
        .stdout(predicate::str::contains("Clarify output wording"));

    assert!(
        fs::read_to_string(output_dir.join("audit-prompt.txt"))?.contains("Linked Linear Issue")
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_review_remediation_path_creates_follow_up_pr_and_linear_comment()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let agent_stub = temp.path().join("review-agent");
    let gh_stub = temp.path().join("gh");
    let output_dir = temp.path().join("output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&output_dir)?;
    fs::create_dir_all(&repo_root)?;
    fs::write(repo_root.join(".gitignore"), "target/\n")?;
    init_repo_with_origin(&repo_root)?;
    write_minimal_planning_context(
        &repo_root,
        r#"{"linear":{"team":"ENG","project_id":"project-1"}}"#,
    )?;
    fs::write(repo_root.join("README.md"), "# Demo\n")?;
    run_git_cli(&repo_root, &["add", "README.md"])?;
    run_git_cli(&repo_root, &["commit", "-m", "Add README"])?;
    run_git_cli(&repo_root, &["push", "origin", "main"])?;
    run_git_cli(&repo_root, &["checkout", "-b", "feature/12345"])?;
    fs::write(repo_root.join("src.txt"), "before\n")?;
    run_git_cli(&repo_root, &["add", "src.txt"])?;
    run_git_cli(&repo_root, &["commit", "-m", "Feature work"])?;
    run_git_cli(&repo_root, &["push", "-u", "origin", "feature/12345"])?;
    run_git_cli(&repo_root, &["checkout", "main"])?;

    write_review_agent_config(&config_path, &api_url, &agent_stub, false)?;
    write_executable(
        &agent_stub,
        r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/agent-count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
cat > "$TEST_OUTPUT_DIR/prompt-$count.txt"
if [ "$count" -eq 1 ]; then
  printf '%s' '{"summary":"The PR still needs a remediation proof.","requires_follow_up_changes":true,"blocking_issues":[{"title":"Missing remediation command test","rationale":"The ticket requires a remediation-required proof.","evidence":["No command-level remediation test exists"],"suggested_fix":"Add a mocked end-to-end remediation test."}],"optional_improvements":[],"broader_impact_areas":["README review workflow docs"],"suggested_validation":["cargo test --test agents_review"],"remediation_summary":"Add the missing command-level remediation proof."}'
else
  printf '\nreview remediation\n' >> README.md
  printf '%s' 'Applied the missing remediation proof and updated docs.'
fi
"#,
    )?;
    write_review_gh_stub(
        &gh_stub,
        "ENG-10251",
        true,
        "https://github.com/metastack-systems/metastack-cli/pull/20000",
    )?;

    let issue = review_issue_node(
        "issue-1",
        "ENG-10251",
        "Add review command",
        "# Acceptance Criteria\n\n- A remediation-required flow creates a follow-up PR\n",
        "state-2",
        "In Progress",
    );
    mock_issue_lookup(&server, vec![issue.clone()]);
    mock_review_issue_detail(
        &server,
        "issue-1",
        review_issue_detail_node(
            "issue-1",
            "ENG-10251",
            "Add review command",
            "# Acceptance Criteria\n\n- A remediation-required flow creates a follow-up PR\n",
            Vec::new(),
        ),
    );
    let create_comment = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateComment");
        then.status(200).json_body(json!({
            "data": {
                "commentCreate": {
                    "success": true,
                    "comment": {
                        "id": "comment-1",
                        "body": "created"
                    }
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .env(
            "PATH",
            format!("{}:{}", temp.path().display(), std::env::var("PATH")?),
        )
        .args([
            "agents",
            "review",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "12345",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Required follow-up changes: yes"))
        .stdout(predicate::str::contains(
            "https://github.com/metastack-systems/metastack-cli/pull/20000",
        ));

    create_comment.assert_calls(1);
    assert!(fs::read_to_string(output_dir.join("prompt-2.txt"))?.contains("Blocking Issues"));

    Ok(())
}

#[cfg(unix)]
fn write_review_agent_config(
    config_path: &Path,
    api_url: &str,
    stub_path: &Path,
    route_override: bool,
) -> Result<(), Box<dyn Error>> {
    let routing = if route_override {
        format!(
            r#"
[agents.routing.commands."agents.review"]
provider = "route-review-stub"
model = "route-model"
reasoning = "high"

[agents.commands.route-review-stub]
command = "{}"
transport = "stdin"
"#,
            stub_path.display()
        )
    } else {
        String::new()
    };
    fs::write(
        config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[onboarding]
completed = true

[agents]
default_agent = "default-review-stub"
default_model = "default-model"
default_reasoning = "medium"

[agents.commands.default-review-stub]
command = "{}"
transport = "stdin"
{}
"#,
            stub_path.display(),
            routing
        ),
    )?;
    Ok(())
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn write_review_gh_stub(
    path: &Path,
    identifier_hint: &str,
    allow_create: bool,
    create_url: &str,
) -> Result<(), Box<dyn Error>> {
    let create_branch = if allow_create {
        format!(
            r#"if [ "$1" = "pr" ] && [ "$2" = "create" ]; then
  printf '%s' '{{"url":"{create_url}"}}'
  exit 0
fi"#
        )
    } else {
        r#"if [ "$1" = "pr" ] && [ "$2" = "create" ]; then
  printf '%s\n' 'unexpected pr create' >&2
  exit 1
fi"#
        .to_string()
    };

    fs::write(
        path,
        format!(
            r#"#!/bin/sh
set -eu
if [ "$1" = "repo" ] && [ "$2" = "view" ]; then
  printf '%s' '{{"nameWithOwner":"metastack-systems/metastack-cli","url":"https://github.com/metastack-systems/metastack-cli"}}'
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  printf '%s' '{{"number":12345,"title":"{identifier_hint} review flow","body":"Implements {identifier_hint}.","url":"https://github.com/metastack-systems/metastack-cli/pull/12345","headRefName":"feature/12345","baseRefName":"main","headRefOid":"abc1234","reviewDecision":"REVIEW_REQUIRED","changedFiles":2,"additions":10,"deletions":2,"author":{{"login":"kames"}},"files":[{{"path":"src/cli.rs","additions":5,"deletions":1}},{{"path":"README.md","additions":5,"deletions":1}}],"comments":[{{"author":{{"login":"reviewer"}},"body":"Looks good overall."}}],"reviews":[{{"author":{{"login":"bot"}},"body":"Please add a remediation proof.","state":"CHANGES_REQUESTED"}}]}}'
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "diff" ]; then
  printf '%s\n' 'diff --git a/src/cli.rs b/src/cli.rs'
  exit 0
fi
if [ "$1" = "api" ]; then
  printf '%s' '[{{"path":"src/cli.rs","body":"Inline note","diff_hunk":"@@ -1 +1 @@","user":{{"login":"inline-reviewer"}}}}]'
  exit 0
fi
{create_branch}
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
fn mock_issue_lookup(server: &MockServer, issues: Vec<serde_json::Value>) {
    server.mock(move |when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": issues
                }
            }
        }));
    });
}

#[cfg(unix)]
fn mock_review_issue_detail(server: &MockServer, issue_id: &str, issue: serde_json::Value) {
    server.mock(move |when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue")
            .body_includes(format!("\"id\":\"{issue_id}\""));
        then.status(200).json_body(json!({
            "data": {
                "issue": issue
            }
        }));
    });
}

#[cfg(unix)]
fn review_issue_detail_node(
    id: &str,
    identifier: &str,
    title: &str,
    description: &str,
    comments: Vec<serde_json::Value>,
) -> serde_json::Value {
    json!({
        "id": id,
        "identifier": identifier,
        "title": title,
        "description": description,
        "url": format!("https://linear.app/issues/{identifier}"),
        "priority": 2,
        "updatedAt": "2026-03-14T16:00:00Z",
        "team": {
            "id": "team-1",
            "key": "ENG",
            "name": "Engineering"
        },
        "project": {
            "id": "project-1",
            "name": "MetaStack CLI"
        },
        "labels": { "nodes": [] },
        "comments": { "nodes": comments },
        "state": {
            "id": "state-2",
            "name": "In Progress",
            "type": "started"
        },
        "attachments": { "nodes": [] },
        "parent": null,
        "children": { "nodes": [] }
    })
}

#[cfg(unix)]
fn review_issue_node(
    id: &str,
    identifier: &str,
    title: &str,
    description: &str,
    state_id: &str,
    state_name: &str,
) -> serde_json::Value {
    json!({
        "id": id,
        "identifier": identifier,
        "title": title,
        "description": description,
        "url": format!("https://linear.app/issues/{identifier}"),
        "priority": 2,
        "updatedAt": "2026-03-14T16:00:00Z",
        "team": {
            "id": "team-1",
            "key": "ENG",
            "name": "Engineering"
        },
        "project": {
            "id": "project-1",
            "name": "MetaStack CLI"
        },
        "state": {
            "id": state_id,
            "name": state_name,
            "type": if state_name == "Todo" { "unstarted" } else { "started" }
        }
    })
}

#[cfg(unix)]
fn run_git_cli(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()?;
    assert!(status.success(), "git {:?} failed", args);
    Ok(())
}
