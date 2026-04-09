use crate::linear::IssueSummary;

use super::PendingLinearSync;
use super::workspace::{TicketWorkspace, TicketWorkspaceProvisioning};

const REVIEW_NOTES_HEADING: &str = "### Review Notes";
const CONTEXT_CHECKPOINT_HEADING: &str = "#### Context Checkpoint";

pub fn render_bootstrap_workpad(
    issue: &IssueSummary,
    workspace: &TicketWorkspace,
    timestamp: &str,
) -> String {
    let plan_requirements = extract_requirements(issue.description.as_deref());
    let acceptance = if plan_requirements.is_empty() {
        vec![
            format!(
                "Implement the requested behavior for `{}` in the dedicated ticket workspace.",
                issue.identifier
            ),
            "Keep a single persistent `## Codex Workpad` comment updated throughout execution."
                .to_string(),
            "Validate the changed behavior with direct command-path proofs before review."
                .to_string(),
        ]
    } else {
        plan_requirements.clone()
    };

    let mut lines = vec![
        "## Codex Workpad".to_string(),
        String::new(),
        "```text".to_string(),
        format!(
            "{}:{}@{}",
            local_hostname(),
            workspace.workspace_path.display(),
            workspace.head_sha
        ),
        "```".to_string(),
        String::new(),
        "### Plan".to_string(),
        String::new(),
        format!(
            "- [ ] 1\\. Reproduce the current behavior and confirm the scope for `{}`",
            issue.identifier
        ),
        format!(
            "  - [ ] 1.1 Capture a deterministic reproduction signal for `{}`",
            issue.identifier
        ),
        "  - [ ] 1.2 Inventory the affected code paths and constraints before editing".to_string(),
        format!(
            "- [ ] 2\\. Complete the local backlog for `{}` in the dedicated workspace clone",
            issue.identifier
        ),
    ];

    if plan_requirements.is_empty() {
        lines.extend([
            "  - [ ] 2.1 Build the feature and config changes described in the issue".to_string(),
            "  - [ ] 2.2 Keep the workpad current as implementation milestones land".to_string(),
        ]);
    } else {
        for (index, item) in plan_requirements.iter().take(4).enumerate() {
            lines.push(format!("  - [ ] 2.{} {}", index + 1, item));
        }
    }

    lines.extend([
        "- [ ] 3\\. Validate, publish, and prepare the change for review".to_string(),
        "  - [ ] 3.1 Run focused tests plus required quality gates".to_string(),
        "  - [ ] 3.2 Commit and push the branch so shared automation can publish the draft PR"
            .to_string(),
        String::new(),
        "### Acceptance Criteria".to_string(),
        String::new(),
    ]);

    for item in acceptance.iter().take(6) {
        lines.push(format!("- [ ] {item}"));
    }
    lines.extend([
        format!(
            "- [ ] Work is executed from `{}` instead of the source repository checkout.",
            workspace.workspace_path.display()
        ),
        format!(
            "- [ ] Local backlog `{}/backlog/{}` stays in sync with the work completed for `{}`.",
            crate::branding::PROJECT_DIR,
            issue.identifier,
            issue.identifier
        ),
        String::new(),
        "### Validation".to_string(),
        String::new(),
        "- [ ] targeted tests: `cargo test`".to_string(),
        "- [ ] quality gates: `cargo fmt --check`".to_string(),
        "- [ ] lint: `cargo clippy --all-targets --all-features -- -D warnings`".to_string(),
        "- [ ] command-path proof: run the changed CLI flow against a deterministic local or mocked setup".to_string(),
        String::new(),
        "### Notes".to_string(),
        String::new(),
        format!(
            "- {timestamp} Prepared working branch `{}` from `{}` in workspace `{}`.",
            workspace.branch,
            workspace.base_ref,
            workspace.workspace_path.display()
        ),
        format!(
            "- {timestamp} Local backlog for `{}` is tracked at `{}/backlog/{}`.",
            issue.identifier,
            crate::branding::PROJECT_DIR,
            issue.identifier
        ),
        format!(
            "- {timestamp} Workspace root: `{}`.",
            workspace.workspace_root.display()
        ),
        format!(
            "- {timestamp} Workspace {} at HEAD `{}`.",
            match workspace.provisioning {
                TicketWorkspaceProvisioning::Created => "created",
                TicketWorkspaceProvisioning::Refreshed => "refreshed",
                TicketWorkspaceProvisioning::Recreated => "recreated",
            },
            workspace.head_sha
        ),
    ]);

    lines.join("\n")
}

pub(crate) fn extract_requirements(description: Option<&str>) -> Vec<String> {
    let Some(description) = description else {
        return Vec::new();
    };

    let mut requirements = Vec::new();
    let mut in_code_block = false;

    for raw_line in description.lines() {
        let line = raw_line.trim();
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block || line.is_empty() {
            continue;
        }

        if let Some(item) = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| numbered_item(line))
        {
            let cleaned = clean_requirement(item);
            if !cleaned.is_empty() {
                requirements.push(cleaned);
            }
            continue;
        }

        if line.starts_with("We also need")
            || line.starts_with("During the")
            || line.starts_with("At times I only want")
            || line.starts_with("We'll have")
        {
            let cleaned = clean_requirement(line);
            if !cleaned.is_empty() {
                requirements.push(cleaned);
            }
        }
    }

    requirements.sort();
    requirements.dedup();
    requirements
}

fn numbered_item(line: &str) -> Option<&str> {
    let (prefix, rest) = line.split_once('.')?;
    prefix
        .chars()
        .all(|ch| ch.is_ascii_digit())
        .then_some(rest.trim())
}

fn clean_requirement(line: &str) -> String {
    line.replace('`', "")
        .trim_matches(|ch: char| ch == '-' || ch == ':' || ch.is_whitespace())
        .to_string()
}

fn local_hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

pub(crate) fn effective_workpad_body(
    issue: &IssueSummary,
    pending_linear_sync: Option<&PendingLinearSync>,
    workpad_comment_id: &str,
) -> Option<String> {
    pending_linear_sync
        .and_then(|pending| pending.workpad_body.clone())
        .or_else(|| {
            issue
                .comments
                .iter()
                .find(|comment| comment.id == workpad_comment_id)
                .map(|comment| comment.body.clone())
        })
        .or_else(|| {
            issue.comments.iter().find_map(|comment| {
                (comment.resolved_at.is_none() && comment.body.contains("## Codex Workpad"))
                    .then(|| comment.body.clone())
            })
        })
}

pub(crate) fn context_checkpoint_present(body: &str) -> bool {
    extract_context_checkpoint(body).is_some()
}

pub(crate) fn extract_context_checkpoint(body: &str) -> Option<String> {
    let review_notes = extract_top_level_section_lines(body, REVIEW_NOTES_HEADING)?;
    let start = review_notes
        .iter()
        .position(|line| line.trim() == CONTEXT_CHECKPOINT_HEADING)?;
    let end = review_notes
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| {
            let trimmed = line.trim();
            trimmed.starts_with("#### ") || trimmed.starts_with("### ")
        })
        .map(|(index, _)| index)
        .unwrap_or(review_notes.len());
    join_trimmed_lines(&review_notes[start..end])
}

pub(crate) fn extract_unmanaged_review_notes(body: &str) -> Vec<String> {
    let Some(review_notes) = extract_top_level_section_lines(body, REVIEW_NOTES_HEADING) else {
        return Vec::new();
    };
    let mut unmanaged = Vec::new();
    let mut in_checkpoint = false;
    for line in review_notes {
        let trimmed = line.trim();
        if trimmed == CONTEXT_CHECKPOINT_HEADING {
            in_checkpoint = true;
            continue;
        }
        if in_checkpoint && (trimmed.starts_with("#### ") || trimmed.starts_with("### ")) {
            in_checkpoint = false;
        }
        if in_checkpoint || is_managed_review_note_line(trimmed) {
            continue;
        }
        unmanaged.push(line);
    }
    trim_blank_lines_owned(unmanaged)
}

pub(crate) fn extract_workpad_section_lines(body: &str, heading: &str) -> Vec<String> {
    extract_top_level_section_lines(body, heading).unwrap_or_default()
}

fn extract_top_level_section_lines(body: &str, heading: &str) -> Option<Vec<String>> {
    let lines = body.lines().map(str::to_string).collect::<Vec<_>>();
    let start = lines.iter().position(|line| line.trim() == heading)?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.trim().starts_with("### "))
        .map(|(index, _)| index)
        .unwrap_or(lines.len());
    Some(trim_blank_lines_owned(lines[start + 1..end].to_vec()))
}

fn is_managed_review_note_line(line: &str) -> bool {
    line.starts_with("- Risk:") || line.starts_with("- Note:")
}

fn join_trimmed_lines(lines: &[String]) -> Option<String> {
    let trimmed = trim_blank_lines(lines);
    (!trimmed.is_empty()).then(|| trimmed.join("\n"))
}

fn trim_blank_lines(lines: &[String]) -> &[String] {
    let start = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(start);
    &lines[start..end]
}

fn trim_blank_lines_owned(lines: Vec<String>) -> Vec<String> {
    trim_blank_lines(&lines).to_vec()
}

#[cfg(test)]
mod tests {
    use crate::linear::{IssueComment, IssueSummary, TeamRef};

    use super::{
        CONTEXT_CHECKPOINT_HEADING, context_checkpoint_present, effective_workpad_body,
        extract_context_checkpoint, extract_unmanaged_review_notes, extract_workpad_section_lines,
    };
    use crate::listen::PendingLinearSync;

    fn issue_with_comments(body: &str) -> IssueSummary {
        IssueSummary {
            id: "issue-1".to_string(),
            identifier: "ENG-10782".to_string(),
            title: "Context pressure".to_string(),
            description: None,
            url: "https://linear.app/issues/ENG-10782".to_string(),
            priority: None,
            estimate: None,
            updated_at: "2026-04-08T00:00:00Z".to_string(),
            team: TeamRef {
                id: "team-1".to_string(),
                key: "ENG".to_string(),
                name: "Engineering".to_string(),
            },
            project: None,
            assignee: None,
            labels: Vec::new(),
            comments: vec![IssueComment {
                id: "comment-1".to_string(),
                body: body.to_string(),
                created_at: None,
                user_name: None,
                resolved_at: None,
            }],
            state: None,
            attachments: Vec::new(),
            parent: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn effective_workpad_body_prefers_pending_sync_body() {
        let issue = issue_with_comments("## Codex Workpad\n\nstored");
        let pending = PendingLinearSync {
            workpad_body: Some("## Codex Workpad\n\npending".to_string()),
            ..PendingLinearSync::default()
        };

        assert_eq!(
            effective_workpad_body(&issue, Some(&pending), "comment-1").as_deref(),
            Some("## Codex Workpad\n\npending")
        );
    }

    #[test]
    fn context_checkpoint_helpers_use_review_notes_section() {
        let body = format!(
            "## Codex Workpad\n\n### Completed\n\n- [x] done\n\n### Review Notes\n\n- Risk: generated note\n- Keep this manual note\n\n{CONTEXT_CHECKPOINT_HEADING}\n\n- Pressure: high\n\n### Validation\n\n- [ ] cargo test\n"
        );

        assert!(context_checkpoint_present(&body));
        assert_eq!(
            extract_context_checkpoint(&body).as_deref(),
            Some("#### Context Checkpoint\n\n- Pressure: high")
        );
        assert_eq!(
            extract_unmanaged_review_notes(&body),
            vec!["- Keep this manual note".to_string()]
        );
        assert_eq!(
            extract_workpad_section_lines(&body, "### Completed"),
            vec!["- [x] done".to_string()]
        );
    }
}
