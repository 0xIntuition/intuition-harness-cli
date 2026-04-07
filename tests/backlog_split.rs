#![allow(dead_code, unused_imports)]

include!("support/common.rs");
use metastack_cli::branding;

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
fn write_split_test_repo(repo_root: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(repo_root)?;
    write_minimal_planning_context(
        repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  }
}
"#,
    )?;
    fs::create_dir_all(repo_root.join(format!("{}/backlog/_TEMPLATE", branding::PROJECT_DIR)))?;
    fs::write(
        repo_root.join(format!(
            "{}/backlog/_TEMPLATE/index.md",
            branding::PROJECT_DIR
        )),
        "# {{issue_title}}\n\nSeeded from the split template.\n",
    )?;
    fs::write(
        repo_root.join(format!(
            "{}/backlog/_TEMPLATE/specification.md",
            branding::PROJECT_DIR
        )),
        "# Specification: {{issue_title}}\n\nSource: {{issue_url}}\n",
    )?;
    fs::write(
        repo_root.join(format!(
            "{}/backlog/_TEMPLATE/validation.md",
            branding::PROJECT_DIR
        )),
        format!(
            "# Validation\n\n- `{} backlog split MET-35`\n",
            branding::COMMAND_NAME
        ),
    )?;
    Ok(())
}

#[cfg(unix)]
fn write_split_agent_stub(stub_path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        stub_path,
        r#"#!/bin/sh
cat > "$TEST_OUTPUT_DIR/payload.txt"
cat <<'JSON'
{
  "summary": "Split the command rollout into implementation and documentation tickets.",
  "child_issues": [
    {
      "proposal_id": "child-1",
      "title": "Implement `meta backlog split` command internals",
      "description": "Build the command path, proposal contract, and apply flow.",
      "acceptance_criteria": [
        "The split command can generate and apply a proposal",
        "Child packets are written for each created issue"
      ],
      "priority": 2
    },
    {
      "proposal_id": "child-2",
      "title": "Document and harden `meta backlog split`",
      "description": "Update docs and regression coverage once the core command works.",
      "acceptance_criteria": [
        "README help text matches the shipped command surface",
        "Regression coverage keeps backlog tech stable"
      ],
      "priority": 3
    }
  ],
  "parent_rewrite": {
    "title": "Implement `meta backlog split` as a tracked umbrella parent",
    "description": "Tracks the split rollout across the core command and the docs and regression pass.",
    "acceptance_criteria": [
      "Child issues cover the full feature scope",
      "The parent issue reflects the umbrella status after the split"
    ]
  },
  "dependency_suggestions": [
    {
      "blocking": "child-1",
      "blocked": "child-2",
      "rationale": "The docs and regression pass should land after the command contract stabilizes."
    }
  ]
}
JSON
"#,
    )?;
    let mut permissions = fs::metadata(stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(stub_path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn write_split_agent_stub_with_addendum_failure(stub_path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        stub_path,
        r#"#!/bin/sh
payload_path="$TEST_OUTPUT_DIR/payload.txt"
cat > "$payload_path"
if grep -q "Operator addendum for refinement" "$payload_path"; then
  printf '%s\n' 'not valid json'
  exit 0
fi
cat <<'JSON'
{
  "summary": "Split the command rollout into implementation and documentation tickets.",
  "child_issues": [
    {
      "proposal_id": "child-1",
      "title": "Implement `meta backlog split` command internals",
      "description": "Build the command path, proposal contract, and apply flow.",
      "acceptance_criteria": [
        "The split command can generate and apply a proposal",
        "Child packets are written for each created issue"
      ],
      "priority": 2
    },
    {
      "proposal_id": "child-2",
      "title": "Document and harden `meta backlog split`",
      "description": "Update docs and regression coverage once the core command works.",
      "acceptance_criteria": [
        "README help text matches the shipped command surface",
        "Regression coverage keeps backlog tech stable"
      ],
      "priority": 3
    }
  ],
  "parent_rewrite": {
    "title": "Implement `meta backlog split` as a tracked umbrella parent",
    "description": "Tracks the split rollout across the core command and the docs and regression pass.",
    "acceptance_criteria": [
      "Child issues cover the full feature scope",
      "The parent issue reflects the umbrella status after the split"
    ]
  },
  "dependency_suggestions": [
    {
      "blocking": "child-1",
      "blocked": "child-2",
      "rationale": "The docs and regression pass should land after the command contract stabilizes."
    }
  ]
}
JSON
"#,
    )?;
    let mut permissions = fs::metadata(stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(stub_path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn write_split_config(config_path: &Path, stub_path: &Path) -> Result<(), Box<dyn Error>> {
    write_onboarded_config(
        config_path,
        format!(
            r#"[agents]
default_agent = "split-stub"

[agents.commands.split-stub]
command = "{}"
transport = "stdin"
"#,
            stub_path.display()
        ),
    )
}

#[cfg(unix)]
fn split_issue_detail() -> serde_json::Value {
    issue_detail_node(
        "parent-1",
        "MET-35",
        "Implement `meta backlog split`",
        "## Context\n\nSplit this issue into a command implementation and a docs hardening follow-up.",
        Vec::new(),
        None,
    )
}

#[cfg(unix)]
fn project_payload() -> serde_json::Value {
    json!({
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
    })
}

#[cfg(unix)]
fn child_issue(
    identifier: &str,
    title: &str,
    state_id: &str,
    state_name: &str,
) -> serde_json::Value {
    issue_detail_node(
        &format!("id-{identifier}"),
        identifier,
        title,
        &format!("# {title}\n\nCreated by backlog split."),
        Vec::new(),
        Some(json!({
            "id": "parent-1",
            "identifier": "MET-35",
            "title": "Implement `meta backlog split`",
            "url": "https://linear.app/issues/MET-35",
            "description": "Parent issue"
        })),
    )
    .as_object()
    .cloned()
    .map(|mut node| {
        node.insert(
            "state".to_string(),
            json!({
                "id": state_id,
                "name": state_name,
                "type": if state_name == "Todo" { "unstarted" } else { "started" }
            }),
        );
        serde_json::Value::Object(node)
    })
    .expect("child issue node should stay an object")
}

#[cfg(unix)]
#[test]
fn backlog_split_no_interactive_emits_structured_proposal_json() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("split-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    write_split_test_repo(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    write_split_agent_stub(&stub_path)?;
    write_split_config(&config_path, &stub_path)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [issue_node(
                        "parent-1",
                        "MET-35",
                        "Implement `meta backlog split`",
                        "Split the parent issue.",
                        "state-2",
                        "In Progress"
                    )]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue")
            .body_includes("\"id\":\"parent-1\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": split_issue_detail()
            }
        }));
    });

    let assert = cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "split",
            "MET-35",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--no-interactive",
        ])
        .assert()
        .success();

    let payload: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout)?;
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["command"], "backlog.split");
    assert_eq!(payload["result"]["applied"], false);
    assert_eq!(payload["result"]["source_issue"]["identifier"], "MET-35");
    assert_eq!(
        payload["result"]["child_issues"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        payload["result"]["parent_rewrite"]["title"],
        "Implement `meta backlog split` as a tracked umbrella parent"
    );
    assert_eq!(
        payload["result"]["dependency_suggestions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_split_render_once_shows_review_flow_snapshot() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("split-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    write_split_test_repo(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    write_split_agent_stub(&stub_path)?;
    write_split_config(&config_path, &stub_path)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [issue_node(
                        "parent-1",
                        "MET-35",
                        "Implement `meta backlog split`",
                        "Split the parent issue.",
                        "state-2",
                        "In Progress"
                    )]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue")
            .body_includes("\"id\":\"parent-1\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": split_issue_detail()
            }
        }));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "split",
            "MET-35",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--render-once",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Split Summary"))
        .stdout(predicate::str::contains("Source Issue"))
        .stdout(predicate::str::contains("Proposed children: 2"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_split_render_once_events_can_apply_split_end_to_end() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("split-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    write_split_test_repo(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    write_split_agent_stub(&stub_path)?;
    write_split_config(&config_path, &stub_path)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [issue_node(
                        "parent-1",
                        "MET-35",
                        "Implement `meta backlog split`",
                        "Split the parent issue.",
                        "state-2",
                        "In Progress"
                    )]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue")
            .body_includes("\"id\":\"parent-1\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": split_issue_detail()
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
            .body_includes("query Projects");
        then.status(200).json_body(project_payload());
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query IssueLabels");
        then.status(200).json_body(json!({
            "data": {
                "issueLabels": {
                    "nodes": [{
                        "id": "label-plan",
                        "name": "plan"
                    }]
                }
            }
        }));
    });

    let child_one_create = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue")
            .body_includes("\"labelIds\":[\"label-plan\"]")
            .body_includes("Implement `meta backlog split` command internals");
        then.status(200).json_body(json!({
            "data": {
                "issueCreate": {
                    "success": true,
                    "issue": child_issue(
                        "MET-36",
                        "Implement `meta backlog split` command internals",
                        "state-1",
                        "Todo"
                    )
                }
            }
        }));
    });

    let child_two_create = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue")
            .body_includes("\"labelIds\":[\"label-plan\"]")
            .body_includes("Document and harden `meta backlog split`");
        then.status(200).json_body(json!({
            "data": {
                "issueCreate": {
                    "success": true,
                    "issue": child_issue(
                        "MET-37",
                        "Document and harden `meta backlog split`",
                        "state-1",
                        "Todo"
                    )
                }
            }
        }));
    });

    let parent_update = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue")
            .body_includes("\"id\":\"parent-1\"")
            .body_includes("Implement `meta backlog split` as a tracked umbrella parent");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": issue_detail_node(
                        "parent-1",
                        "MET-35",
                        "Implement `meta backlog split` as a tracked umbrella parent",
                        "# Implement `meta backlog split` as a tracked umbrella parent\n\nTracks the split rollout across the core command and the docs and regression pass.\n",
                        Vec::new(),
                        None
                    )
                }
            }
        }));
    });

    let relation_create = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssueRelation")
            .body_includes("\"issueId\":\"id-MET-36\"")
            .body_includes("\"relatedIssueId\":\"id-MET-37\"")
            .body_includes("\"type\":\"blocks\"");
        then.status(200).json_body(json!({
            "data": {
                "issueRelationCreate": {
                    "success": true
                }
            }
        }));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "split",
            "MET-35",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--render-once",
            "--events",
            "enter,enter,enter,enter,enter",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Split MET-35 into 2 child issue(s)",
        ))
        .stdout(predicate::str::contains("Backlog packets:"))
        .stdout(predicate::str::contains(format!(
            "{}/backlog/MET-36",
            branding::PROJECT_DIR
        )))
        .stdout(predicate::str::contains(format!(
            "{}/backlog/MET-37",
            branding::PROJECT_DIR
        )));

    child_one_create.assert();
    child_two_create.assert();
    parent_update.assert();
    relation_create.assert();

    let child_one_dir = repo_root.join(format!("{}/backlog/MET-36", branding::PROJECT_DIR));
    let child_two_dir = repo_root.join(format!("{}/backlog/MET-37", branding::PROJECT_DIR));
    assert!(child_one_dir.join("index.md").is_file());
    assert!(child_two_dir.join("index.md").is_file());
    assert!(child_one_dir.join("specification.md").is_file());
    assert!(child_two_dir.join("validation.md").is_file());

    let child_one_index = fs::read_to_string(child_one_dir.join("index.md"))?;
    let child_two_index = fs::read_to_string(child_two_dir.join("index.md"))?;
    assert!(child_one_index.contains("Implement `meta backlog split` command internals"));
    assert!(child_two_index.contains("Document and harden `meta backlog split`"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_split_render_once_can_deselect_dependency_links() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("split-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    write_split_test_repo(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    write_split_agent_stub(&stub_path)?;
    write_split_config(&config_path, &stub_path)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [issue_node(
                        "parent-1",
                        "MET-35",
                        "Implement `meta backlog split`",
                        "Split the parent issue.",
                        "state-2",
                        "In Progress"
                    )]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue")
            .body_includes("\"id\":\"parent-1\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": split_issue_detail()
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
            .body_includes("query Projects");
        then.status(200).json_body(project_payload());
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query IssueLabels");
        then.status(200).json_body(json!({
            "data": {
                "issueLabels": {
                    "nodes": [{
                        "id": "label-plan",
                        "name": "plan"
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue")
            .body_includes("Implement `meta backlog split` command internals");
        then.status(200).json_body(json!({
            "data": {
                "issueCreate": {
                    "success": true,
                    "issue": child_issue(
                        "MET-36",
                        "Implement `meta backlog split` command internals",
                        "state-1",
                        "Todo"
                    )
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue")
            .body_includes("Document and harden `meta backlog split`");
        then.status(200).json_body(json!({
            "data": {
                "issueCreate": {
                    "success": true,
                    "issue": child_issue(
                        "MET-37",
                        "Document and harden `meta backlog split`",
                        "state-1",
                        "Todo"
                    )
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue")
            .body_includes("\"id\":\"parent-1\"");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": issue_detail_node(
                        "parent-1",
                        "MET-35",
                        "Implement `meta backlog split` as a tracked umbrella parent",
                        "# Implement `meta backlog split` as a tracked umbrella parent\n\nTracks the split rollout across the core command and the docs and regression pass.\n",
                        Vec::new(),
                        None
                    )
                }
            }
        }));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "split",
            "MET-35",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--render-once",
            "--events",
            "enter,enter,space,enter,enter,enter",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "parent rewritten as MET-35 and 0 dependency link(s) created",
        ))
        .stdout(predicate::str::contains("Dependency notes:").not());

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_split_render_once_surfaces_addendum_refinement_errors() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("split-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    write_split_test_repo(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    write_split_agent_stub_with_addendum_failure(&stub_path)?;
    write_split_config(&config_path, &stub_path)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [issue_node(
                        "parent-1",
                        "MET-35",
                        "Implement `meta backlog split`",
                        "Split the parent issue.",
                        "state-2",
                        "In Progress"
                    )]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue")
            .body_includes("\"id\":\"parent-1\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": split_issue_detail()
            }
        }));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "split",
            "MET-35",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--render-once",
            "--events",
            "enter,enter,enter,paste=Please tighten the proposal,enter",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Operator Guidance"))
        .stdout(predicate::str::contains(
            "split agent returned invalid JSON during split proposal generation",
        ));

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_split_apply_failure_reports_created_children_when_parent_rewrite_fails()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("split-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    write_split_test_repo(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    write_split_agent_stub(&stub_path)?;
    write_split_config(&config_path, &stub_path)?;

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [issue_node(
                        "parent-1",
                        "MET-35",
                        "Implement `meta backlog split`",
                        "Split the parent issue.",
                        "state-2",
                        "In Progress"
                    )]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue")
            .body_includes("\"id\":\"parent-1\"");
        then.status(200).json_body(json!({
            "data": {
                "issue": split_issue_detail()
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
            .body_includes("query Projects");
        then.status(200).json_body(project_payload());
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query IssueLabels");
        then.status(200).json_body(json!({
            "data": {
                "issueLabels": {
                    "nodes": [{
                        "id": "label-plan",
                        "name": "plan"
                    }]
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue")
            .body_includes("Implement `meta backlog split` command internals");
        then.status(200).json_body(json!({
            "data": {
                "issueCreate": {
                    "success": true,
                    "issue": child_issue(
                        "MET-36",
                        "Implement `meta backlog split` command internals",
                        "state-1",
                        "Todo"
                    )
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssue")
            .body_includes("Document and harden `meta backlog split`");
        then.status(200).json_body(json!({
            "data": {
                "issueCreate": {
                    "success": true,
                    "issue": child_issue(
                        "MET-37",
                        "Document and harden `meta backlog split`",
                        "state-1",
                        "Todo"
                    )
                }
            }
        }));
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue")
            .body_includes("\"id\":\"parent-1\"");
        then.status(200).json_body(json!({
            "errors": [{
                "message": "parent rewrite denied"
            }]
        }));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "split",
            "MET-35",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--render-once",
            "--events",
            "enter,enter,enter,enter,enter",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "created split children [MET-36, MET-37] but failed to rewrite parent `MET-35`",
        ))
        .stderr(predicate::str::contains(
            "Linear request failed: parent rewrite denied",
        ));

    assert!(
        repo_root
            .join(format!("{}/backlog/MET-36/index.md", branding::PROJECT_DIR))
            .is_file()
    );
    assert!(
        repo_root
            .join(format!("{}/backlog/MET-37/index.md", branding::PROJECT_DIR))
            .is_file()
    );

    Ok(())
}
