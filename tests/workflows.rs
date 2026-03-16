#![allow(dead_code, unused_imports)]

include!("support/common.rs");

#[test]
fn workflows_list_shows_builtin_playbooks() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    fs::create_dir_all(&repo_root)?;

    cli()
        .args([
            "workflows",
            "list",
            "--root",
            repo_root.to_string_lossy().as_ref(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Available workflows"))
        .stdout(predicate::str::contains("backlog-planning"))
        .stdout(predicate::str::contains("ticket-implementation"))
        .stdout(predicate::str::contains("pr-review"))
        .stdout(predicate::str::contains("incident-triage"));

    Ok(())
}

#[test]
fn workflows_explain_describes_ticket_implementation_contract() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    fs::create_dir_all(&repo_root)?;

    cli()
        .args([
            "workflows",
            "explain",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "ticket-implementation",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Workflow: ticket-implementation"))
        .stdout(predicate::str::contains("Linear issue parameter: `issue`"))
        .stdout(predicate::str::contains("implementation_notes"))
        .stdout(predicate::str::contains("Validation"))
        .stdout(predicate::str::contains("Prompt Template"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn workflows_run_resolves_linear_issue_and_executes_selected_provider() -> Result<(), Box<dyn Error>>
{
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(repo_root.join("src"))?;
    fs::create_dir_all(repo_root.join("instructions"))?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    fs::write(
        repo_root.join("Cargo.toml"),
        r#"[package]
name = "workflow-demo"
version = "0.1.0"
edition = "2024"
"#,
    )?;
    fs::write(repo_root.join("README.md"), "# Workflow Demo\n")?;
    fs::write(repo_root.join("src/main.rs"), "fn main() {}\n")?;
    fs::write(
        repo_root.join("AGENTS.md"),
        "# Repo Rules\nUse focused validation.\n",
    )?;
    fs::write(
        repo_root.join("instructions/listen.md"),
        "# Listener Instructions\nKeep the workpad current.\n",
    )?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "instructions_path": "instructions/listen.md"
  }
}
"#,
    )?;
    fs::write(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents.commands.workflow-stub]
command = "workflow-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;

    let stub_path = bin_dir.join("workflow-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$METASTACK_AGENT_PROMPT" > "$TEST_OUTPUT_DIR/prompt.txt"
printf '%s' "$METASTACK_AGENT_INSTRUCTIONS" > "$TEST_OUTPUT_DIR/instructions.txt"
printf 'workflow stub ok'
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;

    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [issue_node(
                        "issue-93",
                        "MET-93",
                        "Introduce reusable workflow playbooks",
                        "Add workflow and context commands",
                        "state-2",
                        "In Progress"
                    )]
                }
            }
        }));
    });
    let detail_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-93",
                    "MET-93",
                    "Introduce reusable workflow playbooks",
                    "Add workflow and context commands",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new()
                )
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
            "workflows",
            "run",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "ticket-implementation",
            "--provider",
            "workflow-stub",
            "--param",
            "issue=MET-93",
            "--param",
            "implementation_notes=Focus on repeatable CLI behavior.",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Ran workflow `ticket-implementation`",
        ))
        .stdout(predicate::str::contains("workflow stub ok"));

    issues_mock.assert_calls(1);
    detail_mock.assert_calls(1);
    let prompt = fs::read_to_string(stub_dir.join("prompt.txt"))?;
    let instructions = fs::read_to_string(stub_dir.join("instructions.txt"))?;
    assert!(prompt.contains("Introduce reusable workflow playbooks"));
    assert!(prompt.contains("Focus on repeatable CLI behavior."));
    assert!(prompt.contains("## Built-in Workflow Contract"));
    assert!(prompt.contains("Repo Rules"));
    assert!(instructions.contains("senior engineer preparing to implement"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn workflows_run_uses_route_specific_provider_defaults() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let bin_dir = temp.path().join("bin");
    let stub_dir = temp.path().join("stub-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    fs::create_dir_all(repo_root.join("src"))?;
    fs::create_dir_all(repo_root.join("instructions"))?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&stub_dir)?;

    fs::write(
        repo_root.join("Cargo.toml"),
        r#"[package]
name = "workflow-demo"
version = "0.1.0"
edition = "2024"
"#,
    )?;
    fs::write(repo_root.join("README.md"), "# Workflow Demo\n")?;
    fs::write(repo_root.join("src/main.rs"), "fn main() {}\n")?;
    fs::write(
        repo_root.join("instructions/listen.md"),
        "# Listener Instructions\nKeep the workpad current.\n",
    )?;
    write_minimal_planning_context(
        &repo_root,
        r#"{
  "linear": {
    "team": "MET",
    "project_id": "project-1"
  },
  "listen": {
    "instructions_path": "instructions/listen.md"
  }
}
"#,
    )?;
    fs::write(
        &config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[agents]
default_agent = "codex"
default_model = "gpt-5.4"

[agents.routing.commands."agents.workflows.run"]
provider = "workflow-stub"

[agents.commands.workflow-stub]
command = "workflow-stub"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        ),
    )?;

    let stub_path = bin_dir.join("workflow-stub");
    fs::write(
        &stub_path,
        r#"#!/bin/sh
printf '%s' "$METASTACK_AGENT_NAME" > "$TEST_OUTPUT_DIR/agent.txt"
printf 'workflow route stub ok'
"#,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;

    let issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [issue_node(
                        "issue-93",
                        "MET-93",
                        "Introduce reusable workflow playbooks",
                        "Add workflow and context commands",
                        "state-2",
                        "In Progress"
                    )]
                }
            }
        }));
    });
    let detail_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue($id: String!)");
        then.status(200).json_body(json!({
            "data": {
                "issue": listen_issue_detail_node(
                    "issue-93",
                    "MET-93",
                    "Introduce reusable workflow playbooks",
                    "Add workflow and context commands",
                    "state-2",
                    "In Progress",
                    Vec::new(),
                    Vec::new(),
                    Vec::new()
                )
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
            "workflows",
            "run",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "ticket-implementation",
            "--param",
            "issue=MET-93",
            "--param",
            "implementation_notes=Route-specific defaults should win.",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("provider `workflow-stub`"))
        .stdout(predicate::str::contains("workflow route stub ok"));

    assert_eq!(
        fs::read_to_string(stub_dir.join("agent.txt"))?,
        "workflow-stub"
    );
    issues_mock.assert_calls(1);
    detail_mock.assert_calls(1);

    Ok(())
}

#[test]
fn unsupported_provider_model_combination_returns_actionable_error() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    fs::create_dir_all(repo_root.join(".metastack/workflows"))?;
    fs::write(
        repo_root.join(".metastack/workflows/invalid-provider.md"),
        r#"---
name: invalid-provider
summary: Minimal workflow for model validation.
provider: codex
---

Validate provider/model compatibility.
"#,
    )?;

    meta()
        .current_dir(&repo_root)
        .args([
            "workflows",
            "run",
            "--root",
            repo_root.to_str().expect("temp path should be utf-8"),
            "invalid-provider",
            "--provider",
            "codex",
            "--model",
            "opus",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "model `opus` is not supported for agent `codex`",
        ))
        .stderr(predicate::str::contains("supported models"));

    Ok(())
}
