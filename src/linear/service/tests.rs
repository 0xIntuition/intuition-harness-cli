use std::sync::atomic::Ordering;

use super::{
    LinearService,
    test_support::{FakeLinearClient, comment, issue, issue_for_team, label, project, team},
};
use crate::linear::{IssueCreateSpec, IssueEditSpec, IssueListFilters};

#[tokio::test]
async fn list_issues_full_scans_and_applies_filters() {
    let client = FakeLinearClient {
        issues: vec![issue("MET-01", "Todo", Some("project-1"), "MetaStack CLI")],
        all_issues: vec![
            issue("MET-12", "In Progress", Some("project-1"), "MetaStack CLI"),
            issue_for_team(
                "OPS-02",
                "OPS",
                "In Progress",
                Some("project-2"),
                "Operations",
            ),
            issue("MET-11", "Todo", Some("project-1"), "MetaStack CLI"),
        ],
        ..FakeLinearClient::default()
    };
    let service = LinearService::new(client.clone(), Some("MET".to_string()));

    let issues = service
        .list_issues(IssueListFilters {
            state: Some("In Progress".to_string()),
            limit: 5,
            ..IssueListFilters::default()
        })
        .await
        .expect("list issues should succeed");

    assert_eq!(
        issues
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["MET-12"]
    );
    assert_eq!(client.list_all_issues_calls.load(Ordering::SeqCst), 1);
    assert_eq!(client.list_issues_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn create_issue_resolves_team_state_and_project_for_team() {
    let client = FakeLinearClient {
        teams: vec![team(
            "MET",
            &[("state-1", "Todo"), ("state-2", "In Progress")],
        )],
        projects: vec![
            project("project-1", "MetaStack CLI", &["MET"]),
            project("project-2", "Other", &["OPS"]),
        ],
        ..FakeLinearClient::default()
    };
    let service = LinearService::new(client.clone(), None);

    service
        .create_issue(IssueCreateSpec {
            team: Some("MET".to_string()),
            title: "Refactor boundaries".to_string(),
            description: Some("Split handlers and services".to_string()),
            project: Some("MetaStack CLI".to_string()),
            project_id: None,
            parent_id: None,
            state: Some("In Progress".to_string()),
            priority: Some(2),
            labels: Vec::new(),
        })
        .await
        .expect("create issue should succeed");

    let requests = client.create_requests.lock().expect("mutex poisoned");
    let request = requests
        .first()
        .expect("a create request should be recorded");
    assert_eq!(request.team_id, "team-MET");
    assert_eq!(request.project_id.as_deref(), Some("project-1"));
    assert_eq!(request.state_id.as_deref(), Some("state-2"));
    assert_eq!(request.priority, Some(2));
}

#[tokio::test]
async fn create_issue_keeps_explicit_project_id_when_lookup_misses() {
    let client = FakeLinearClient {
        teams: vec![team("MET", &[("state-1", "Todo")])],
        projects: vec![project("project-1", "MetaStack CLI", &["MET"])],
        ..FakeLinearClient::default()
    };
    let service = LinearService::new(client.clone(), None);

    service
        .create_issue(IssueCreateSpec {
            team: Some("MET".to_string()),
            title: "Use seeded project".to_string(),
            description: None,
            project: None,
            project_id: Some("project-seeded".to_string()),
            parent_id: None,
            state: Some("Todo".to_string()),
            priority: None,
            labels: Vec::new(),
        })
        .await
        .expect("create issue should succeed");

    let requests = client.create_requests.lock().expect("mutex poisoned");
    let request = requests
        .first()
        .expect("a create request should be recorded");
    assert_eq!(request.project_id.as_deref(), Some("project-seeded"));
}

#[tokio::test]
async fn ensure_issue_labels_exist_creates_only_missing_labels() {
    let client = FakeLinearClient {
        teams: vec![team("MET", &[("state-1", "Todo")])],
        issue_labels: vec![label("label-plan", "plan")],
        ..FakeLinearClient::default()
    };
    let service = LinearService::new(client.clone(), Some("MET".to_string()));

    service
        .ensure_issue_labels_exist(
            None,
            &[
                "plan".to_string(),
                "technical".to_string(),
                "TECHNICAL".to_string(),
            ],
        )
        .await
        .expect("labels should be reconciled");

    let created = client.created_issue_labels.lock().expect("mutex poisoned");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].team_id, "team-MET");
    assert_eq!(created[0].name, "technical");
}

#[tokio::test]
async fn resolve_project_selector_strict_rejects_ambiguous_matches() {
    let client = FakeLinearClient {
        projects: vec![
            project("project-1", "MetaStack CLI", &["MET"]),
            project("project-2", "MetaStack CLI", &["MET", "OPS"]),
        ],
        ..FakeLinearClient::default()
    };
    let service = LinearService::new(client, None);

    let error = service
        .resolve_project_selector_strict("MetaStack CLI", Some("MET"))
        .await
        .expect_err("ambiguous projects should fail");

    assert!(
        error
            .to_string()
            .contains("matched multiple Linear projects")
    );
    assert!(error.to_string().contains("project-1"));
    assert!(error.to_string().contains("project-2"));
}

#[tokio::test]
async fn edit_issue_updates_requested_fields_after_loading_context() {
    let client = FakeLinearClient {
        issues: vec![issue("MET-11", "Todo", Some("project-1"), "MetaStack CLI")],
        all_issues: vec![issue("MET-11", "Todo", Some("project-1"), "MetaStack CLI")],
        teams: vec![team(
            "MET",
            &[("state-1", "Todo"), ("state-2", "In Progress")],
        )],
        projects: vec![project("project-1", "MetaStack CLI", &["MET"])],
        updated_issue: Some(issue(
            "MET-11",
            "In Progress",
            Some("project-1"),
            "MetaStack CLI",
        )),
        ..FakeLinearClient::default()
    };
    let service = LinearService::new(client.clone(), Some("MET".to_string()));

    let issue = service
        .edit_issue(IssueEditSpec {
            identifier: "MET-11".to_string(),
            title: Some("CLI Foundation".to_string()),
            description: None,
            project: Some("MetaStack CLI".to_string()),
            state: Some("In Progress".to_string()),
            priority: Some(1),
        })
        .await
        .expect("issue edit should succeed");

    assert_eq!(
        issue.state.as_ref().map(|state| state.name.as_str()),
        Some("In Progress")
    );
    let updates = client.update_requests.lock().expect("mutex poisoned");
    let (issue_id, request) = updates
        .first()
        .expect("an update request should be recorded");
    assert_eq!(issue_id, "issue-met-11");
    assert_eq!(request.title.as_deref(), Some("CLI Foundation"));
    assert_eq!(request.project_id.as_deref(), Some("project-1"));
    assert_eq!(request.state_id.as_deref(), Some("state-2"));
    assert_eq!(request.priority, Some(1));
}

#[tokio::test]
async fn upsert_workpad_comment_updates_existing_active_comment() {
    let client = FakeLinearClient::default();
    let service = LinearService::new(client.clone(), None);
    let mut issue = issue("MET-11", "Todo", Some("project-1"), "MetaStack CLI");
    issue.comments = vec![
        comment(
            "comment-resolved",
            "## Codex Workpad",
            Some("2026-03-15T00:00:00Z"),
        ),
        comment("comment-active", "## Codex Workpad\n\nold body", None),
    ];

    let updated = service
        .upsert_workpad_comment(&issue, "## Codex Workpad\n\nnew body".to_string())
        .await
        .expect("existing workpad should update");

    assert_eq!(updated.id, "comment-active");
    let updates = client.updated_comments.lock().expect("mutex poisoned");
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, "comment-active");
}
