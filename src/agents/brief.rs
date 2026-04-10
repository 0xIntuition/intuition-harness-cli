use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::fs::{PlanningPaths, canonicalize_existing_dir, ensure_dir, write_text_file};
use crate::listen::extract_ticket_inline_sections;
use crate::scaffold::ensure_planning_layout;

#[derive(Debug, Clone, Default)]
pub(crate) struct TicketMetadata {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) url: Option<String>,
    pub(crate) state: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentBriefRequest {
    pub(crate) ticket: String,
    pub(crate) title_override: Option<String>,
    pub(crate) goal: Option<String>,
    pub(crate) metadata: TicketMetadata,
    pub(crate) output: Option<PathBuf>,
}

pub(crate) fn write_agent_brief(root: &Path, request: AgentBriefRequest) -> Result<PathBuf> {
    let root = canonicalize_existing_dir(root)?;
    ensure_planning_layout(&root, false)?;
    let paths = PlanningPaths::new(&root);
    ensure_dir(&paths.agent_briefs_dir)?;

    let output_path = request.output.clone().unwrap_or_else(|| {
        paths
            .agent_briefs_dir
            .join(format!("{}.md", sanitize_ticket(&request.ticket)))
    });
    let contents = render_brief(&request)?;
    write_text_file(&output_path, &contents, true)?;

    Ok(output_path)
}

fn render_brief(request: &AgentBriefRequest) -> Result<String> {
    let title = request
        .metadata
        .title
        .clone()
        .or_else(|| request.title_override.clone())
        .unwrap_or_else(|| "Title unavailable".to_string());

    let mut lines = vec![
        format!("# Agent Kickoff: {}", request.ticket),
        String::new(),
        "## Objective".to_string(),
        String::new(),
        format!("- Ticket: `{}`", request.ticket),
        format!("- Title: {}", title),
    ];

    if let Some(goal) = request.goal.as_deref() {
        lines.push(format!("- Goal: {}", goal));
    }

    if let Some(state) = &request.metadata.state {
        lines.push(format!("- Current state: {}", state));
    }

    if let Some(url) = &request.metadata.url {
        lines.push(format!("- Linear URL: {}", url));
    }

    lines.extend([
        String::new(),
        "## Guidance".to_string(),
        String::new(),
        format!(
            "- Use `{}/codebase/*.md` as the reusable source of repo context when those files are present.",
            crate::branding::PROJECT_DIR
        ),
        "- Capture reproduction, implement the requested change, validate with focused command proofs, and update the workpad.".to_string(),
        String::new(),
        "## Codebase Context".to_string(),
        String::new(),
        codebase_reference_line("Scan", "SCAN.md"),
        codebase_reference_line("Architecture", "ARCHITECTURE.md"),
        codebase_reference_line("Concerns", "CONCERNS.md"),
        codebase_reference_line("Conventions", "CONVENTIONS.md"),
        codebase_reference_line("Integrations", "INTEGRATIONS.md"),
        codebase_reference_line("Stack", "STACK.md"),
        codebase_reference_line("Structure", "STRUCTURE.md"),
        codebase_reference_line("Testing", "TESTING.md"),
    ]);

    for section in extract_ticket_inline_sections(request.metadata.description.as_deref()) {
        lines.push(String::new());
        lines.push(section.render());
    }

    Ok(lines.join("\n"))
}

fn sanitize_ticket(ticket: &str) -> String {
    ticket
        .chars()
        .map(|character| match character {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' => character,
            _ => '-',
        })
        .collect()
}

fn codebase_reference_line(label: &str, file_name: &str) -> String {
    format!(
        "- {label}: `{}/codebase/{file_name}`",
        crate::branding::PROJECT_DIR
    )
}
