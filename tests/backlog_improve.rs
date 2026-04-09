#![allow(dead_code, unused_imports)]

include!("support/common.rs");
use metastack_cli::branding;

#[cfg(unix)]
#[test]
fn backlog_improve_scans_repo_backlog_and_writes_proposal_artifacts() -> Result<(), Box<dyn Error>>
{
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("backlog-improve-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
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
    write_backlog_improve_config(&config_path, &api_url, &stub_path)?;
    write_backlog_improve_stub(
        &stub_path,
        &format!(
            "#!/bin/sh\ncat > \"$TEST_OUTPUT_DIR/payload-1.txt\"\ncat <<'JSON'\n{{\"summary\":\"Needs a small cleanup before execution.\",\"needs_improvement\":true,\"findings\":{{\"title_gaps\":[\"Title should say what gets improved.\"],\"description_gaps\":[\"Acceptance criteria are missing from the body.\"],\"acceptance_criteria_gaps\":[\"Add executable acceptance criteria.\"],\"metadata_gaps\":[\"Missing the planning label and estimate.\"],\"structure_opportunities\":[]}},\"proposal\":{{\"title\":\"Improve backlog hygiene workflow\",\"description\":\"# Improve backlog hygiene workflow\\n\\n## Acceptance Criteria\\n\\n- `{cmd} backlog improve` scans repo backlog issues\\n- Proposal artifacts are stored under `{dir}/backlog/MET-510/artifacts/improvement/`\\n\",\"priority\":2,\"estimate\":3,\"labels\":[\"plan\",\" plan \"],\"acceptance_criteria\":[\"`{cmd} backlog improve` scans repo backlog issues\",\"Proposal artifacts are stored under `{dir}/backlog/MET-510/artifacts/improvement/`\"]}}}}\nJSON\n",
            cmd = branding::COMMAND_NAME,
            dir = branding::PROJECT_DIR,
        ),
    )?;

    let issue_dir = repo_root.join(format!("{}/backlog/MET-510", branding::PROJECT_DIR));
    fs::create_dir_all(&issue_dir)?;
    fs::write(issue_dir.join("index.md"), "# Existing local packet\n")?;

    mock_issue_list(
        &server,
        vec![
            issue_node(
                "issue-510",
                "MET-510",
                "Backlog cleanup",
                "Current description",
                "state-backlog",
                "Backlog",
            ),
            issue_node(
                "issue-511",
                "MET-511",
                "Sibling backlog item",
                "Sibling description",
                "state-backlog",
                "Backlog",
            ),
        ],
    );
    let update_issue_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": issue_node(
                        "issue-510",
                        "MET-510",
                        "Improve backlog hygiene workflow",
                        "Current description",
                        "state-backlog",
                        "Backlog",
                    )
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "improve",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "--limit",
            "5",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Improved 2 issue(s):"))
        .stdout(predicate::str::contains("MET-510: basic proposal only"))
        .stdout(predicate::str::contains("MET-511: basic proposal only"));

    let run_dir = latest_improvement_dir(&issue_dir)?;
    assert_eq!(
        fs::read_to_string(run_dir.join("original.md"))?,
        "Current description"
    );
    assert_eq!(
        fs::read_to_string(run_dir.join("local-index.md"))?,
        "# Existing local packet\n"
    );
    assert!(fs::read_to_string(run_dir.join("proposal.md"))?.contains("## Proposed Changes"));
    let summary = fs::read_to_string(run_dir.join("summary.json"))?;
    assert!(summary.contains("\"needs_improvement\": true"));
    assert!(summary.contains("\"requested\": false"));
    assert!(summary.contains("\"local_updated\": false"));
    assert!(summary.contains("\"remote_updated\": false"));
    assert_eq!(
        fs::read_to_string(issue_dir.join("index.md"))?,
        "# Existing local packet\n"
    );

    let payload = fs::read_to_string(output_dir.join("payload-1.txt"))?;
    assert!(payload.contains("Improvement mode: `basic`"));
    assert!(payload.contains("Current local backlog index snapshot:"));
    assert!(payload.contains("Related repo-scoped backlog issues:"));
    update_issue_mock.assert_calls(0);

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_improve_apply_updates_local_packet_and_linear_issue() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("backlog-improve-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
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
    write_backlog_improve_config(&config_path, &api_url, &stub_path)?;
    write_backlog_improve_stub(
        &stub_path,
        &format!(
            "#!/bin/sh\ncat > \"$TEST_OUTPUT_DIR/payload-1.txt\"\nprintf '%s' \
'{{\"summary\":\"Ready to apply.\",\"needs_improvement\":true,\
\"findings\":{{\"title_gaps\":[],\"description_gaps\":[],\"acceptance_criteria_gaps\":[],\
\"metadata_gaps\":[\"Set an estimate before execution.\"],\
\"structure_opportunities\":[\"Parenting is already fine.\"]}},\
\"proposal\":{{\"title\":\"Applied backlog improvement\",\
\"description\":\"# Applied backlog improvement\\n\\n## Acceptance Criteria\\n\\n\
- `{cmd} backlog improve MET-610 --mode advanced --apply` updates the local packet before Linear\\n\",\
\"priority\":1,\"estimate\":5,\
\"acceptance_criteria\":[\"`{cmd} backlog improve MET-610 --mode advanced --apply` updates the local packet before Linear\"]}}}}'\n",
            cmd = branding::COMMAND_NAME,
        ),
    )?;

    let issue_dir = repo_root.join(format!("{}/backlog/MET-610", branding::PROJECT_DIR));
    fs::create_dir_all(&issue_dir)?;
    fs::write(issue_dir.join("index.md"), "# Previous local packet\n")?;

    let issue = issue_node(
        "issue-610",
        "MET-610",
        "Old backlog title",
        "Remote description before apply",
        "state-backlog",
        "Backlog",
    );
    mock_issue_list(&server, vec![issue.clone()]);
    mock_issue_detail(
        &server,
        "issue-610",
        issue_detail_node(
            "issue-610",
            "MET-610",
            "Old backlog title",
            "Remote description before apply",
            Vec::new(),
            None,
        ),
    );
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });
    let update_issue_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue")
            .body_includes("\"id\":\"issue-610\"")
            .body_includes("Applied backlog improvement")
            .body_includes("\"priority\":1")
            .body_includes("\"estimate\":5.0");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": issue_node(
                        "issue-610",
                        "MET-610",
                        "Applied backlog improvement",
                        "# Applied backlog improvement",
                        "state-backlog",
                        "Backlog",
                    )
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "improve",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "MET-610",
            "--mode",
            "advanced",
            "--apply",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-610: advanced applied"));

    assert_eq!(
        fs::read_to_string(issue_dir.join("index.md"))?,
        format!(
            "# Applied backlog improvement\n\n## Acceptance Criteria\n\n- `{} backlog improve MET-610 --mode advanced --apply` updates the local packet before Linear",
            branding::COMMAND_NAME
        )
    );
    let run_dir = latest_improvement_dir(&issue_dir)?;
    assert_eq!(
        fs::read_to_string(run_dir.join("applied-local-before.md"))?,
        "# Previous local packet\n"
    );
    assert_eq!(
        fs::read_to_string(run_dir.join("applied-remote-before.md"))?,
        "Remote description before apply"
    );
    let summary = fs::read_to_string(run_dir.join("summary.json"))?;
    assert!(summary.contains("\"requested\": true"));
    assert!(summary.contains("\"local_updated\": true"));
    assert!(summary.contains("\"remote_updated\": true"));
    assert!(summary.contains("\"mode\": \"advanced\""));
    update_issue_mock.assert_calls(1);

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_improve_apply_without_project_override_uses_paged_linear_connections()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("backlog-improve-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
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
    write_backlog_improve_config(&config_path, &api_url, &stub_path)?;
    write_backlog_improve_stub(
        &stub_path,
        &format!(
            "#!/bin/sh\nprintf '%s' \
'{{\"summary\":\"Ready to apply.\",\"needs_improvement\":true,\
\"findings\":{{\"title_gaps\":[],\"description_gaps\":[],\"acceptance_criteria_gaps\":[],\
\"metadata_gaps\":[\"Set an estimate before execution.\"],\"structure_opportunities\":[]}},\
\"proposal\":{{\"title\":\"Regression-proof backlog improvement\",\
\"description\":\"# Regression-proof backlog improvement\\n\\n## Acceptance Criteria\\n\\n\
- `{cmd} backlog improve MET-611 --mode advanced --apply` succeeds without `--project`\\n\",\
\"priority\":2,\"estimate\":3,\
\"acceptance_criteria\":[\"`{cmd} backlog improve MET-611 --mode advanced --apply` succeeds without `--project`\"]}}}}'\n",
            cmd = branding::COMMAND_NAME,
        ),
    )?;

    let issue_dir = repo_root.join(format!("{}/backlog/MET-611", branding::PROJECT_DIR));
    fs::create_dir_all(&issue_dir)?;
    fs::write(issue_dir.join("index.md"), "# Existing local packet\n")?;

    let issue = issue_node(
        "issue-611",
        "MET-611",
        "Regression test ticket",
        "Remote description before apply",
        "state-backlog",
        "Backlog",
    );
    let issue_list_mock = server.mock({
        let issue = issue.clone();
        move |when, then| {
            when.method(POST)
                .path("/graphql")
                .body_includes("query Issues")
                .body_includes("labels(first: 50)");
            then.status(200).json_body(json!({
                "data": {
                    "issues": {
                        "nodes": [issue]
                    }
                }
            }));
        }
    });
    let issue_detail_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Issue")
            .body_includes("\"id\":\"issue-611\"")
            .body_includes("labels(first: 50)");
        then.status(200).json_body(json!({
            "data": {
                "issue": issue_detail_node(
                    "issue-611",
                    "MET-611",
                    "Regression test ticket",
                    "Remote description before apply",
                    Vec::new(),
                    None,
                )
            }
        }));
    });
    let teams_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams")
            .body_includes("states(first: 50)");
        then.status(200).json_body(team_payload());
    });
    let update_issue_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue")
            .body_includes("\"id\":\"issue-611\"")
            .body_includes("labels(first: 50)");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": issue_node(
                        "issue-611",
                        "MET-611",
                        "Regression-proof backlog improvement",
                        "# Regression-proof backlog improvement",
                        "state-backlog",
                        "Backlog",
                    )
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "improve",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "MET-611",
            "--mode",
            "advanced",
            "--apply",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-611: advanced applied"));

    issue_list_mock.assert_calls(3);
    issue_detail_mock.assert_calls(1);
    teams_mock.assert_calls(1);
    update_issue_mock.assert_calls(1);

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_improve_claude_permission_failure_classifies_error() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("backlog-improve-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
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
    write_backlog_improve_config(&config_path, &api_url, &stub_path)?;
    // The agent stub returns a valid improvement proposal (with ANSI escape sequences
    // injected to verify they are stripped before any terminal output).
    write_backlog_improve_stub(
        &stub_path,
        r##"#!/bin/sh
cat > "$TEST_OUTPUT_DIR/payload-1.txt"
printf '\033[1m%s\033[0m' '{"summary":"Ready to apply.","needs_improvement":true,"route":"ready_for_update","findings":{"title_gaps":[],"description_gaps":[],"acceptance_criteria_gaps":[],"metadata_gaps":["Set an estimate."],"structure_opportunities":[]},"proposal":{"title":"Permission test improvement","description":"# Permission test improvement\n\nUpdated description.\n","priority":2,"estimate":3,"acceptance_criteria":["Permission test passes"]}}'
"##,
    )?;

    let issue_dir = repo_root.join(format!("{}/backlog/MET-710", branding::PROJECT_DIR));
    fs::create_dir_all(&issue_dir)?;
    fs::write(issue_dir.join("index.md"), "# Original local packet\n")?;

    let issue = issue_node(
        "issue-710",
        "MET-710",
        "Permission test ticket",
        "Original remote description",
        "state-backlog",
        "Backlog",
    );
    mock_issue_list(&server, vec![issue.clone()]);
    mock_issue_detail(
        &server,
        "issue-710",
        issue_detail_node(
            "issue-710",
            "MET-710",
            "Permission test ticket",
            "Original remote description",
            Vec::new(),
            None,
        ),
    );
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });
    // Mock the issueUpdate mutation to return a permission-denied GraphQL error.
    let update_issue_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue");
        then.status(200).json_body(json!({
            "errors": [{
                "message": "You do not have permission to update this issue. Authentication required.",
                "extensions": { "code": "FORBIDDEN" }
            }]
        }));
    });

    let output = meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "improve",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "MET-710",
            "--mode",
            "advanced",
            "--apply",
        ])
        .output()
        .expect("meta command should execute");

    // The command must exit with a non-zero status.
    assert!(
        !output.status.success(),
        "expected non-zero exit code for permission failure"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Error output must classify the failure as a Linear permission error.
    assert!(
        stderr.contains("permission denied") || stderr.contains("permission"),
        "stderr should mention permission failure, got: {stderr}"
    );
    assert!(
        stderr.contains("LINEAR_API_KEY"),
        "stderr should suggest checking LINEAR_API_KEY scopes, got: {stderr}"
    );
    assert!(
        stderr.contains("Local proposal saved"),
        "stderr should mention local proposal was saved, got: {stderr}"
    );

    // Error output must NOT misclassify as "invalid JSON".
    assert!(
        !stderr.contains("invalid JSON"),
        "stderr should not mention invalid JSON for a permission error, got: {stderr}"
    );

    // No raw ANSI escape fragments should appear in stdout or stderr.
    assert!(
        !stdout.contains('\x1b'),
        "stdout should not contain raw ANSI escape sequences"
    );
    assert!(
        !stderr.contains('\x1b'),
        "stderr should not contain raw ANSI escape sequences"
    );

    // Local artifacts should have been created successfully.
    let run_dir = latest_improvement_dir(&issue_dir)?;
    assert!(
        run_dir.join("proposal.json").exists(),
        "proposal.json should exist"
    );
    assert!(
        run_dir.join("proposal.md").exists(),
        "proposal.md should exist"
    );
    assert!(
        run_dir.join("original.md").exists(),
        "original.md should exist"
    );

    // The summary.json should record the error kind as linear_permission.
    let summary = fs::read_to_string(run_dir.join("summary.json"))?;
    assert!(
        summary.contains("\"error_kind\": \"linear_permission\""),
        "summary.json should record error_kind as linear_permission, got: {summary}"
    );
    assert!(
        summary.contains("\"local_updated\": true"),
        "local_updated should be true since proposal had a description"
    );
    assert!(
        summary.contains("\"remote_updated\": false"),
        "remote_updated should be false since Linear mutation failed"
    );

    // The Linear update mutation should have been called.
    update_issue_mock.assert_calls(1);

    Ok(())
}

#[cfg(unix)]
#[test]
fn render_improvement_prompt_does_not_seed_technical_labels() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("backlog-improve-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
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
    write_backlog_improve_config(&config_path, &api_url, &stub_path)?;
    write_backlog_improve_stub(
        &stub_path,
        r#"#!/bin/sh
cat > "$TEST_OUTPUT_DIR/payload-1.txt"
printf '%s' '{"summary":"Looks good.","needs_improvement":false,"route":"no_update_needed","findings":{"title_gaps":[],"description_gaps":[],"acceptance_criteria_gaps":[],"metadata_gaps":[],"structure_opportunities":[]},"proposal":{"acceptance_criteria":[]}}'
"#,
    )?;

    mock_issue_list(
        &server,
        vec![issue_node(
            "issue-614",
            "MET-614",
            "Prompt label proof",
            "Current description",
            "state-backlog",
            "Backlog",
        )],
    );
    mock_issue_detail(
        &server,
        "issue-614",
        issue_detail_node(
            "issue-614",
            "MET-614",
            "Prompt label proof",
            "Current description",
            Vec::new(),
            None,
        ),
    );

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "improve",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "MET-614",
        ])
        .assert()
        .success();

    let payload = fs::read_to_string(output_dir.join("payload-1.txt"))?;
    assert!(payload.contains("\"labels\": [\"plan\"]"));
    assert!(!payload.contains("\"labels\": [\"plan\", \"technical\"]"));

    Ok(())
}

#[cfg(unix)]
#[test]
fn normalize_improvement_output_does_not_introduce_technical_labels_for_plan_tickets()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("backlog-improve-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
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
    write_backlog_improve_config(&config_path, &api_url, &stub_path)?;
    write_backlog_improve_stub(
        &stub_path,
        r#"#!/bin/sh
printf '%s' '{"summary":"Add the plan label only.","needs_improvement":true,"route":"ready_for_update","findings":{"title_gaps":[],"description_gaps":[],"acceptance_criteria_gaps":[],"metadata_gaps":["Add plan label."],"structure_opportunities":[]},"proposal":{"labels":["plan","technical","tech"],"acceptance_criteria":["Keep labels safe"]}}'
"#,
    )?;

    let issue_dir = repo_root.join(format!("{}/backlog/MET-615", branding::PROJECT_DIR));
    fs::create_dir_all(&issue_dir)?;
    fs::write(issue_dir.join("index.md"), "# Existing local packet\n")?;

    mock_issue_list(
        &server,
        vec![issue_node(
            "issue-615",
            "MET-615",
            "Normalize technical labels",
            "Current description",
            "state-backlog",
            "Backlog",
        )],
    );
    mock_issue_detail(
        &server,
        "issue-615",
        issue_detail_node(
            "issue-615",
            "MET-615",
            "Normalize technical labels",
            "Current description",
            Vec::new(),
            None,
        ),
    );

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "improve",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "MET-615",
        ])
        .assert()
        .success();

    let run_dir = latest_improvement_dir(&issue_dir)?;
    let proposal: Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("proposal.json"))?)?;
    assert_eq!(proposal["proposal"]["labels"], json!(["plan"]));

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_improve_apply_preserves_existing_labels_without_injecting_technical()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("backlog-improve-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
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
    write_backlog_improve_config(&config_path, &api_url, &stub_path)?;
    write_backlog_improve_stub(
        &stub_path,
        &format!(
            "#!/bin/sh\nprintf '%s' \
'{{\"summary\":\"Add the planning label but keep the existing issue metadata.\",\
\"needs_improvement\":true,\
\"route\":\"ready_for_update\",\
\"recommendation\":\"Apply the small label cleanup now.\",\
\"findings\":{{\"title_gaps\":[],\"description_gaps\":[],\"acceptance_criteria_gaps\":[],\
\"metadata_gaps\":[\"Add the plan label.\"],\"structure_opportunities\":[]}},\
\"proposal\":{{\"title\":\"Preserve labels during improve apply\",\
\"description\":\"# Preserve labels during improve apply\\n\\n## Acceptance Criteria\\n\\n\
- `{cmd} backlog improve MET-612 --mode advanced --apply` keeps existing labels while adding `plan`\\n\",\
\"priority\":2,\"estimate\":3,\
\"labels\":[\"plan\",\"technical\"],\
\"acceptance_criteria\":[\"`{cmd} backlog improve MET-612 --mode advanced --apply` keeps existing labels while adding `plan`\"]}}}}'\n",
            cmd = branding::COMMAND_NAME,
        ),
    )?;

    let issue_dir = repo_root.join(format!("{}/backlog/MET-612", branding::PROJECT_DIR));
    fs::create_dir_all(&issue_dir)?;
    fs::write(issue_dir.join("index.md"), "# Existing local packet\n")?;

    let issue = issue_node_with_labels(
        "issue-612",
        "MET-612",
        "Preserve labels",
        "Remote description before apply",
        "state-backlog",
        "Backlog",
        &[("label-feature", "feature")],
    );
    mock_issue_list(&server, vec![issue.clone()]);
    mock_issue_detail(
        &server,
        "issue-612",
        issue_detail_node_with_labels(
            "issue-612",
            "MET-612",
            "Preserve labels",
            "Remote description before apply",
            Vec::new(),
            None,
            &[("label-feature", "feature")],
        ),
    );
    server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query Teams");
        then.status(200).json_body(team_payload());
    });
    let _issue_labels_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("query IssueLabels");
        then.status(200).json_body(issue_labels_payload(&[
            ("label-feature", "feature"),
            ("label-plan", "plan"),
        ]));
    });
    let create_technical_label_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation CreateIssueLabel")
            .body_includes("technical");
        then.status(200).json_body(json!({
            "data": {
                "issueLabelCreate": {
                    "success": true,
                    "issueLabel": {
                        "id": "label-technical",
                        "name": "technical"
                    }
                }
            }
        }));
    });
    let update_issue_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateIssue")
            .body_includes("\"id\":\"issue-612\"")
            .body_includes("\"labelIds\":[\"label-feature\",\"label-plan\"]");
        then.status(200).json_body(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": issue_node_with_labels(
                        "issue-612",
                        "MET-612",
                        "Preserve labels during improve apply",
                        "# Preserve labels during improve apply",
                        "state-backlog",
                        "Backlog",
                        &[
                            ("label-feature", "feature"),
                            ("label-plan", "plan")
                        ],
                    )
                }
            }
        }));
    });

    meta()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "improve",
            "--api-key",
            "token",
            "--api-url",
            &api_url,
            "MET-612",
            "--mode",
            "advanced",
            "--apply",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MET-612: advanced applied"));

    let run_dir = latest_improvement_dir(&issue_dir)?;
    let proposal: Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("proposal.json"))?)?;
    assert_eq!(proposal["proposal"]["labels"], json!(["feature", "plan"]));

    create_technical_label_mock.assert_calls(0);
    update_issue_mock.assert_calls(1);

    Ok(())
}

#[cfg(unix)]
#[test]
fn backlog_improve_interactive_cleanup_restores_terminal_state_before_return()
-> Result<(), Box<dyn Error>> {
    let mut output = metastack_cli::backlog_improve_terminal_cleanup_bytes()?;
    let summary = "Improved 1 issue(s):\n- MET-613: accepted no-update recommendation\n";
    let summary_start = output.len();
    output.extend_from_slice(summary.as_bytes());

    assert!(
        output[..summary_start].contains(&0x1b),
        "cleanup proof should include terminal control bytes before the summary"
    );
    assert!(
        !output[summary_start..].contains(&0x1b),
        "summary tail should be free of terminal escape bytes"
    );

    let summary_tail = String::from_utf8_lossy(&output[summary_start..]);
    assert!(summary_tail.contains("MET-613: accepted no-update recommendation"));
    assert!(!summary_tail.to_ascii_lowercase().contains("parse error"));
    assert!(!summary_tail.to_ascii_lowercase().contains("zsh:"));

    Ok(())
}

#[cfg(unix)]
fn write_backlog_improve_config(
    config_path: &Path,
    api_url: &str,
    stub_path: &Path,
) -> Result<(), Box<dyn Error>> {
    fs::write(
        config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"

[onboarding]
completed = true

[agents]
default_agent = "backlog-improve-stub"

[agents.commands.backlog-improve-stub]
command = "{}"
transport = "stdin"
"#,
            stub_path.display()
        ),
    )?;
    Ok(())
}

#[cfg(unix)]
fn write_backlog_improve_stub(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn mock_issue_list(server: &MockServer, issues: Vec<serde_json::Value>) {
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
fn mock_issue_detail(server: &MockServer, issue_id: &str, issue: serde_json::Value) {
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
fn latest_improvement_dir(issue_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let improvement_root = issue_dir.join("artifacts").join("improvement");
    let mut entries = fs::read_dir(&improvement_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    entries.pop().ok_or_else(|| {
        format!(
            "no improvement run found under `{}`",
            improvement_root.display()
        )
        .into()
    })
}

#[cfg(unix)]
fn issue_node_with_labels(
    id: &str,
    identifier: &str,
    title: &str,
    description: &str,
    state_id: &str,
    state_name: &str,
    labels: &[(&str, &str)],
) -> serde_json::Value {
    let mut issue = issue_node(id, identifier, title, description, state_id, state_name);
    issue["labels"] = json!({
        "nodes": labels
            .iter()
            .map(|(id, name)| json!({ "id": id, "name": name }))
            .collect::<Vec<_>>()
    });
    issue
}

#[cfg(unix)]
fn issue_detail_node_with_labels(
    id: &str,
    identifier: &str,
    title: &str,
    description: &str,
    attachments: Vec<serde_json::Value>,
    parent: Option<serde_json::Value>,
    labels: &[(&str, &str)],
) -> serde_json::Value {
    let mut issue = issue_detail_node(id, identifier, title, description, attachments, parent);
    issue["labels"] = json!({
        "nodes": labels
            .iter()
            .map(|(id, name)| json!({ "id": id, "name": name }))
            .collect::<Vec<_>>()
    });
    issue
}

#[cfg(unix)]
fn issue_labels_payload(labels: &[(&str, &str)]) -> serde_json::Value {
    json!({
        "data": {
            "issueLabels": {
                "nodes": labels
                    .iter()
                    .map(|(id, name)| json!({ "id": id, "name": name }))
                    .collect::<Vec<_>>(),
                "pageInfo": {
                    "hasNextPage": false,
                    "endCursor": null
                }
            }
        }
    })
}

#[cfg(unix)]
fn run_backlog_improve_in_pty(
    repo_root: &Path,
    config_path: &Path,
    args: &[&str],
    input: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    let binary = std::env::var_os(format!("CARGO_BIN_EXE_{}", branding::COMMAND_NAME))
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe().ok().and_then(|path| {
                let target_dir = path.parent()?.parent()?;
                [
                    branding::COMMAND_NAME.to_string(),
                    format!("{}.exe", branding::COMMAND_NAME),
                ]
                .into_iter()
                .map(|name| target_dir.join(name))
                .find(|candidate| candidate.is_file())
            })
        })
        .ok_or_else(|| "test binary should be available".to_string())?;

    let command_string = std::iter::once(binary.to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .map(|arg| shell_quote(&arg))
        .collect::<Vec<_>>()
        .join(" ");

    let mut command = ProcessCommand::new("script");
    for key in TEST_ENV_REMOVALS {
        command.env_remove(key);
    }
    let home_dir = isolated_home_dir();
    command
        .current_dir(repo_root)
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", home_dir.join(".config"))
        .env("METASTACK_CONFIG", config_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if cfg!(target_os = "linux") {
        command.args(["-qec", &command_string, "/dev/null"]);
    } else {
        command.args(["-q", "/dev/null", "sh", "-lc", &command_string]);
    }

    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let mut wrote_any = false;
        for chunk in input.split_inclusive('\n') {
            if chunk.is_empty() {
                continue;
            }
            if let Err(error) = stdin.write_all(chunk.as_bytes()) {
                if error.kind() == std::io::ErrorKind::BrokenPipe {
                    break;
                }
                return Err(error.into());
            }
            wrote_any = true;
            std::thread::sleep(Duration::from_millis(1200));
        }
        if !wrote_any && !input.is_empty() {
            if let Err(error) = stdin.write_all(input.as_bytes()) {
                if error.kind() != std::io::ErrorKind::BrokenPipe {
                    return Err(error.into());
                }
            }
        }
    }

    Ok(child.wait_with_output()?)
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
