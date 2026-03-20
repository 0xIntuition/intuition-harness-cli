#![allow(dead_code, unused_imports)]

include!("support/common.rs");

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
fn review_issue_node(
    id: &str,
    identifier: &str,
    title: &str,
    description: &str,
    labels: &[&str],
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
            "key": "MET",
            "name": "Metastack"
        },
        "project": {
            "id": "project-1",
            "name": "MetaStack CLI"
        },
        "assignee": serde_json::Value::Null,
        "labels": {
            "nodes": labels.iter().enumerate().map(|(index, label)| {
                json!({"id": format!("label-{index}"), "name": label})
            }).collect::<Vec<_>>()
        },
        "comments": { "nodes": [] },
        "state": {
            "id": "state-1",
            "name": "Backlog",
            "type": "unstarted"
        },
        "attachments": { "nodes": [] },
        "parent": serde_json::Value::Null,
        "children": { "nodes": [] }
    })
}

#[cfg(unix)]
#[test]
fn backlog_review_noninteractive_json_reports_outcomes_without_mutating_linear()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("review-agent-stub");
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
  "review": {
    "default_states": ["Backlog"],
    "reviewed_label": "reviewed"
  },
  "issue_labels": {
    "plan": "plan"
  }
}
"#,
    )?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[linear]
api_url = "{api_url}"
api_key = "test-token"
team = "MET"

[agents]
default_agent = "review-stub"

[agents.commands.review-stub]
command = "{}"
transport = "stdin"
"#,
            stub_path.display()
        ),
    )?;
    fs::write(
        &stub_path,
        r##"#!/bin/sh
payload=$(cat)
case "$payload" in
  *"ENG-12"*)
    printf '%s' '{"classification":"suggested_edits","summary":"Tighten the implementation contract.","proposed_description":"# Better issue\n\nClarify the repo-scoped CLI behavior.\n"}'
    ;;
  *"ENG-13"*)
    printf '%s' '{"classification":"follow_up_questions","summary":"Missing operator-facing details.","follow_up_questions":["Which repo root should own the command?","Should the review add a label after follow-up is answered?"]}'
    ;;
  *)
    printf '%s' '{"classification":"already_good","summary":"Looks ready."}'
    ;;
esac
"##,
    )?;
    let mut permissions = fs::metadata(&stub_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_path, permissions)?;

    let list_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/graphql")
            .header("authorization", "test-token")
            .body_includes("query Issues");
        then.status(200).json_body(json!({
            "data": {
                "issues": {
                    "nodes": [
                        review_issue_node("issue-11", "ENG-11", "Already reviewed", "Existing ticket", &["reviewed"]),
                        review_issue_node("issue-12", "ENG-12", "Needs edits", "Draft description", &[]),
                        review_issue_node("issue-13", "ENG-13", "Needs questions", "Ambiguous description", &[]),
                        review_issue_node("issue-14", "ENG-14", "Plan-ready", "Planning ticket", &["plan"])
                    ]
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
                    "issue": review_issue_node("issue-99", "ENG-99", "unused", "unused", &[])
                }
            }
        }));
    });

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "backlog",
            "review",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--no-interactive",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"identifier\": \"ENG-11\""))
        .stdout(predicate::str::contains(
            "\"classification\": \"already_reviewed\"",
        ))
        .stdout(predicate::str::contains("\"identifier\": \"ENG-12\""))
        .stdout(predicate::str::contains(
            "\"classification\": \"suggested_edits\"",
        ))
        .stdout(predicate::str::contains(
            "\"proposed_next_action\": \"apply_suggested_description\"",
        ))
        .stdout(predicate::str::contains("\"identifier\": \"ENG-13\""))
        .stdout(predicate::str::contains(
            "\"classification\": \"follow_up_questions\"",
        ))
        .stdout(predicate::str::contains(
            "Which repo root should own the command?",
        ))
        .stdout(predicate::str::contains("\"identifier\": \"ENG-14\""))
        .stdout(predicate::str::contains(
            "\"classification\": \"ready_for_technical_scoping\"",
        ))
        .stdout(predicate::str::contains("meta backlog tech ENG-14"))
        .stdout(predicate::str::contains(
            "Resolved route key: backlog.review",
        ));

    list_mock.assert_calls(1);
    update_mock.assert_calls(0);
    Ok(())
}
