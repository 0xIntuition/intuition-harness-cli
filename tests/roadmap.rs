#![allow(dead_code, unused_imports)]

include!("support/common.rs");

#[cfg(unix)]
fn write_onboarded_config(
    config_path: &Path,
    api_url: &str,
    agent_path: &Path,
) -> Result<(), Box<dyn Error>> {
    fs::write(
        config_path,
        format!(
            r#"[linear]
api_key = "token"
api_url = "{api_url}"
team = "MET"

[agents]
default_agent = "roadmap-stub"

[agents.commands.roadmap-stub]
command = "{}"
transport = "stdin"

[onboarding]
completed = true
"#,
            agent_path.display()
        ),
    )?;
    Ok(())
}

#[cfg(unix)]
fn write_roadmap_agent_stub(stub_path: &Path, proposal: &str) -> Result<(), Box<dyn Error>> {
    fs::write(
        stub_path,
        format!(
            r#"#!/bin/sh
count_file="$TEST_OUTPUT_DIR/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
cat > "$TEST_OUTPUT_DIR/payload-$count.txt"
if [ "$count" -eq 1 ]; then
  printf '%s' '{{"questions":["What release horizon should this roadmap cover?"]}}'
else
  cat <<'EOF'
{proposal}
EOF
fi
"#
        ),
    )?;
    let mut permissions = fs::metadata(stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(stub_path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn roadmap_project_payload(description: &str) -> serde_json::Value {
    json!({
        "data": {
            "project": {
                "id": "project-1",
                "name": "MetaStack CLI",
                "description": description,
                "url": "https://linear.app/project/project-1",
                "progress": 0.42,
                "teams": {
                    "nodes": [{
                        "id": "team-1",
                        "key": "MET",
                        "name": "Metastack"
                    }]
                }
            }
        }
    })
}

#[cfg(unix)]
fn updated_project_payload(description: &str) -> serde_json::Value {
    json!({
        "data": {
            "projectUpdate": {
                "success": true,
                "project": {
                    "id": "project-1",
                    "name": "MetaStack CLI",
                    "description": description,
                    "url": "https://linear.app/project/project-1",
                    "progress": 0.42,
                    "teams": {
                        "nodes": [{
                            "id": "team-1",
                            "key": "MET",
                            "name": "Metastack"
                        }]
                    }
                }
            }
        }
    })
}

#[cfg(unix)]
fn latest_run_dir(repo_root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let mut entries = fs::read_dir(repo_root.join(".metastack/roadmap-runs"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    entries
        .pop()
        .ok_or_else(|| "expected one roadmap run directory".into())
}

#[cfg(unix)]
#[test]
fn roadmap_first_run_creates_repo_file_and_run_artifacts_before_linear_sync()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("roadmap-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    let proposal = "## Summary\n\nShip the canonical roadmap flow.\n\n## Current State\n\n- The backlog family has no roadmap command.\n\n## Near-Term Workstreams\n\n- Add command routing.\n- Persist roadmap run artifacts.\n\n## Risks And Dependencies\n\n- Linear project-doc sync depends on project description support.\n\n## Validation\n\n- `cargo test --test roadmap`";

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(repo_root.join("docs"))?;
    fs::create_dir_all(&output_dir)?;
    fs::write(
        repo_root.join("README.md"),
        "# Demo Repo\n\nRoadmap source.\n",
    )?;
    fs::write(
        repo_root.join("docs/notes.txt"),
        "Important roadmap note.\n",
    )?;
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
    init_repo_with_origin(&repo_root)?;
    write_onboarded_config(&config_path, &api_url, &stub_path)?;
    write_roadmap_agent_stub(&stub_path, proposal)?;

    let project_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "token")
            .body_includes("query Project")
            .body_includes("\"id\":\"project-1\"");
        then.status(200).json_body(roadmap_project_payload(""));
    });
    let done_issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "token")
            .body_includes("query Issues")
            .body_includes("\"project\":{\"id\":{\"eq\":\"project-1\"}}")
            .body_includes("\"state\":{\"name\":{\"eq\":\"Done\"}}");
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
    let update_project_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "token")
            .body_includes("mutation UpdateProject")
            .body_includes("\"id\":\"project-1\"")
            .body_includes("metastack:roadmap:start")
            .body_includes("## Summary");
        then.status(200)
            .json_body(updated_project_payload(&format!("# Roadmap\n\n<!-- metastack:roadmap:start -->\n\n{proposal}\n\n<!-- metastack:roadmap:end -->\n")));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "roadmap",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--request",
            "Refresh the canonical roadmap",
            "--answer",
            "Focus on the next release",
            "--apply",
            "--no-interactive",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("repo=created"))
        .stdout(predicate::str::contains("repo-ahead -> in-sync"));

    let roadmap = fs::read_to_string(repo_root.join("roadmap.md"))?;
    assert!(roadmap.contains("# Roadmap"));
    assert!(roadmap.contains("<!-- metastack:roadmap:start -->"));
    assert!(roadmap.contains("## Near-Term Workstreams"));

    let run_dir = latest_run_dir(&repo_root)?;
    assert!(run_dir.join("source-manifest.json").is_file());
    assert!(run_dir.join("proposal.md").is_file());
    assert!(run_dir.join("summary.json").is_file());

    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("summary.json"))?)?;
    assert_eq!(summary["repo_write_status"].as_str(), Some("created"));
    assert_eq!(summary["pre_sync_divergence"].as_str(), Some("repo-ahead"));
    assert_eq!(summary["post_sync_divergence"].as_str(), Some("in-sync"));
    assert!(summary["apply_error"].is_null());

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("source-manifest.json"))?)?;
    let labels = manifest
        .as_array()
        .expect("manifest should be an array")
        .iter()
        .filter_map(|entry| entry.get("label"))
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    assert!(labels.contains(&"README.md"));
    assert!(labels.contains(&"docs/notes.txt"));

    project_mock.assert_calls(1);
    done_issues_mock.assert_calls(1);
    update_project_mock.assert_calls(1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn roadmap_refresh_run_writes_diff_artifact_without_blind_replacement() -> Result<(), Box<dyn Error>>
{
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("roadmap-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    let current = "# Roadmap\n\n<!-- metastack:roadmap:start -->\n\n## Summary\n\nCurrent roadmap.\n\n## Current State\n\n- Old state.\n\n## Near-Term Workstreams\n\n- Old work.\n\n## Risks And Dependencies\n\n- Old risk.\n\n## Validation\n\n- Old validation.\n\n<!-- metastack:roadmap:end -->\n";
    let proposal = "## Summary\n\nUpdated roadmap.\n\n## Current State\n\n- New state.\n\n## Near-Term Workstreams\n\n- New work.\n\n## Risks And Dependencies\n\n- New risk.\n\n## Validation\n\n- New validation.";

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    fs::write(repo_root.join("README.md"), "# Demo Repo\n")?;
    fs::write(repo_root.join("roadmap.md"), current)?;
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
    init_repo_with_origin(&repo_root)?;
    write_onboarded_config(&config_path, &api_url, &stub_path)?;
    write_roadmap_agent_stub(&stub_path, proposal)?;

    let project_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "token")
            .body_includes("query Project");
        then.status(200).json_body(roadmap_project_payload(current));
    });
    let done_issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "token")
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
    let update_project_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateProject");
        then.status(200).json_body(updated_project_payload(current));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "roadmap",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--request",
            "Refresh the canonical roadmap",
            "--answer",
            "Focus on the next release",
            "--no-interactive",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("repo=not-applied"));

    assert_eq!(fs::read_to_string(repo_root.join("roadmap.md"))?, current);
    let run_dir = latest_run_dir(&repo_root)?;
    assert!(run_dir.join("diff.md").is_file());
    let diff = fs::read_to_string(run_dir.join("diff.md"))?;
    assert!(diff.contains("--- current roadmap"));
    assert!(diff.contains("+++ proposal"));
    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("summary.json"))?)?;
    assert_eq!(summary["repo_write_status"].as_str(), Some("not-applied"));
    assert_eq!(summary["post_sync_divergence"].as_str(), Some("in-sync"));

    project_mock.assert_calls(1);
    done_issues_mock.assert_calls(1);
    update_project_mock.assert_calls(0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn roadmap_apply_refuses_linear_ahead_state_and_records_failure_summary()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("roadmap-agent-stub");
    let output_dir = temp.path().join("agent-output");
    let server = MockServer::start();
    let api_url = server.url("/graphql");
    let repo_roadmap = "# Roadmap\n\n<!-- metastack:roadmap:start -->\n\n## Summary\n\nRepo copy.\n\n## Current State\n\n- Local.\n\n## Near-Term Workstreams\n\n- Local work.\n\n## Risks And Dependencies\n\n- Local risk.\n\n## Validation\n\n- Local validation.\n\n<!-- metastack:roadmap:end -->\n";
    let linear_roadmap = "# Roadmap\n\n<!-- metastack:roadmap:start -->\n\n## Summary\n\nLinear copy.\n\n## Current State\n\n- Remote.\n\n## Near-Term Workstreams\n\n- Remote work.\n\n## Risks And Dependencies\n\n- Remote risk.\n\n## Validation\n\n- Remote validation.\n\n<!-- metastack:roadmap:end -->\n";
    let proposal = "## Summary\n\nProposed new roadmap.\n\n## Current State\n\n- New state.\n\n## Near-Term Workstreams\n\n- New work.\n\n## Risks And Dependencies\n\n- New risk.\n\n## Validation\n\n- New validation.";

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    fs::write(repo_root.join("README.md"), "# Demo Repo\n")?;
    fs::write(repo_root.join("roadmap.md"), repo_roadmap)?;
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
    init_repo_with_origin(&repo_root)?;
    write_onboarded_config(&config_path, &api_url, &stub_path)?;
    write_roadmap_agent_stub(&stub_path, proposal)?;

    let project_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "token")
            .body_includes("query Project");
        then.status(200)
            .json_body(roadmap_project_payload(linear_roadmap));
    });
    let done_issues_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "token")
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
    let update_project_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .body_includes("mutation UpdateProject");
        then.status(200)
            .json_body(updated_project_payload(linear_roadmap));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "backlog",
            "roadmap",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--request",
            "Refresh the canonical roadmap",
            "--answer",
            "Focus on the next release",
            "--apply",
            "--no-interactive",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Linear-ahead")
                .or(predicate::str::contains("Linear project doc")),
        );

    assert_eq!(
        fs::read_to_string(repo_root.join("roadmap.md"))?,
        repo_roadmap
    );
    let run_dir = latest_run_dir(&repo_root)?;
    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("summary.json"))?)?;
    assert_eq!(
        summary["pre_sync_divergence"].as_str(),
        Some("linear-ahead")
    );
    assert_eq!(
        summary["post_sync_divergence"].as_str(),
        Some("linear-ahead")
    );
    assert_eq!(summary["repo_write_status"].as_str(), Some("not-applied"));
    assert!(
        summary["apply_error"]
            .as_str()
            .is_some_and(|value| value.contains("Linear-ahead"))
    );

    project_mock.assert_calls(1);
    done_issues_mock.assert_calls(1);
    update_project_mock.assert_calls(0);
    Ok(())
}
