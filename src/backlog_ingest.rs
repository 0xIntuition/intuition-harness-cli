use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;

use crate::{
    cli::BacklogIngestArgs,
    linear::{IssueCreateSpec, LinearService, ReqwestLinearClient},
    load_linear_command_context,
    output::{MachineIssueSummary, render_json_success},
};

/// Parse and optionally apply a shaped Markdown backlog as Linear issues.
///
/// Returns an error when the input file cannot be read, the backlog has no importable issues, or
/// when `--apply` is set and Linear validation or issue creation fails.
pub(crate) async fn run_backlog_ingest(args: &BacklogIngestArgs) -> Result<String> {
    let contents = fs::read_to_string(&args.path).with_context(|| {
        format!(
            "failed to read backlog ingest input `{}`",
            args.path.display()
        )
    })?;
    let parsed = parse_shaped_backlog(&contents);
    if parsed.issues.is_empty() {
        bail!(
            "no importable backlog issues were found in `{}`",
            args.path.display()
        );
    }

    let mut report = BacklogIngestReport::from_parsed(&args.path, &parsed, args.apply);
    if args.apply {
        let created = apply_backlog(args, &parsed).await?;
        report.created = created
            .iter()
            .map(MachineIssueSummary::from)
            .collect::<Vec<_>>();
    }

    if args.json {
        render_json_success("backlog.ingest", &report)
    } else {
        Ok(render_ingest_report(&report))
    }
}

async fn apply_backlog(
    args: &BacklogIngestArgs,
    parsed: &ParsedBacklog,
) -> Result<Vec<crate::linear::IssueSummary>> {
    let command_context = load_linear_command_context(&args.client, args.team.clone())?;
    let service = command_context.service;
    let team = args.team.clone().or(command_context.default_team);
    let state = args.state.clone().or(command_context.default_state);
    let prepared = prepare_issues_for_apply(
        &service,
        parsed,
        team.as_deref(),
        state.as_deref(),
        args.default_lead.as_deref(),
        args.default_milestone.as_deref(),
    )
    .await?;

    let mut created = Vec::with_capacity(prepared.len());
    for issue in prepared {
        let created_issue = service
            .create_issue(IssueCreateSpec {
                team: team.clone(),
                title: issue.item.title.clone(),
                description: Some(issue.description),
                project: None,
                project_id: Some(issue.project_id),
                project_milestone_id: Some(issue.project_milestone_id),
                parent_id: None,
                state: state.clone(),
                priority: None,
                assignee_id: Some(issue.assignee_id),
                labels: Vec::new(),
            })
            .await
            .with_context(|| {
                format!(
                    "failed to create Linear issue for source line {} (`{}`)",
                    issue.item.source_line, issue.item.title
                )
            })?;
        created.push(created_issue);
    }

    Ok(created)
}

struct PreparedIssue<'a> {
    item: &'a ParsedIssue,
    project_id: String,
    project_milestone_id: String,
    assignee_id: String,
    description: String,
}

async fn prepare_issues_for_apply<'a>(
    service: &LinearService<ReqwestLinearClient>,
    parsed: &'a ParsedBacklog,
    team: Option<&str>,
    state: Option<&str>,
    default_lead: Option<&str>,
    default_milestone: Option<&str>,
) -> Result<Vec<PreparedIssue<'a>>> {
    let mut project_cache = BTreeMap::<String, String>::new();
    let mut milestone_cache = BTreeMap::<(String, String), String>::new();
    let mut assignee_cache = BTreeMap::<String, String>::new();
    let mut prepared = Vec::with_capacity(parsed.issues.len());

    for item in &parsed.issues {
        let project_key = normalize_lookup_key(&item.project);
        let project_id = match project_cache.get(&project_key) {
            Some(project_id) => project_id.clone(),
            None => {
                let resolved = service
                    .resolve_project_selector_strict(&item.project, team)
                    .await
                    .with_context(|| {
                        format!(
                            "failed to resolve project `{}` for source line {}",
                            item.project, item.source_line
                        )
                    })?;
                project_cache.insert(project_key, resolved.clone());
                resolved
            }
        };

        let milestone_name = item
            .milestone
            .as_deref()
            .or(default_milestone)
            .ok_or_else(|| {
                anyhow!(
                    "source line {} (`{}`) has no milestone; pass --default-milestone or add milestone metadata",
                    item.source_line,
                    item.title
                )
            })?;
        let milestone_key = (project_id.clone(), normalize_lookup_key(milestone_name));
        let project_milestone_id = match milestone_cache.get(&milestone_key) {
            Some(milestone_id) => milestone_id.clone(),
            None => {
                let resolved = service
                    .resolve_project_milestone_id_strict(&project_id, &item.project, milestone_name)
                    .await
                    .with_context(|| {
                        format!(
                            "failed to resolve milestone `{milestone_name}` for project `{}` at source line {}",
                            item.project, item.source_line
                        )
                    })?;
                milestone_cache.insert(milestone_key, resolved.clone());
                resolved
            }
        };

        let lead = item.lead.as_deref().or(default_lead).ok_or_else(|| {
            anyhow!(
                "source line {} (`{}`) has no lead; pass --default-lead or add lead metadata",
                item.source_line,
                item.title
            )
        })?;
        let lead_key = normalize_lookup_key(lead);
        let assignee_id = match assignee_cache.get(&lead_key) {
            Some(assignee_id) => assignee_id.clone(),
            None => {
                let resolved = service
                    .resolve_assignee_id(Some(lead))
                    .await?
                    .ok_or_else(|| anyhow!("lead `{lead}` did not resolve to a Linear user"))?;
                assignee_cache.insert(lead_key, resolved.clone());
                resolved
            }
        };

        prepared.push(PreparedIssue {
            item,
            project_id,
            project_milestone_id,
            assignee_id,
            description: render_issue_description(
                item,
                parsed.project_notes.get(&item.project),
                state,
            ),
        });
    }

    Ok(prepared)
}

#[derive(Debug, Clone)]
struct ParsedBacklog {
    issues: Vec<ParsedIssue>,
    project_notes: BTreeMap<String, Vec<String>>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ParsedIssue {
    source_line: usize,
    initiative: Option<String>,
    project: String,
    milestone: Option<String>,
    lead: Option<String>,
    contributors: Vec<String>,
    kind: String,
    title: String,
    raw: String,
}

#[derive(Default)]
struct ParserState {
    initiative: Option<String>,
    project: Option<String>,
    milestone: Option<String>,
    lead: Option<String>,
    contributors: Vec<String>,
}

fn parse_shaped_backlog(markdown: &str) -> ParsedBacklog {
    let mut state = ParserState::default();
    let mut issues = Vec::new();
    let mut project_notes = BTreeMap::<String, Vec<String>>::new();
    let mut warnings = Vec::new();

    for (index, line) in markdown.lines().enumerate() {
        let source_line = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some((level, heading)) = parse_heading(trimmed) {
            apply_heading(&mut state, level, &heading);
            continue;
        }

        if let Some((metadata_key, metadata_value)) = parse_metadata_line(trimmed) {
            apply_metadata(&mut state, &mut project_notes, metadata_key, metadata_value);
            continue;
        }

        let Some(bullet) = strip_bullet_marker(trimmed) else {
            continue;
        };

        if let Some((metadata_key, metadata_value)) = parse_metadata_line(bullet) {
            apply_metadata(&mut state, &mut project_notes, metadata_key, metadata_value);
            continue;
        }

        let Some(project) = state.project.clone() else {
            warnings.push(format!(
                "source line {source_line} is a backlog item outside a project and was skipped"
            ));
            continue;
        };

        let mut item_metadata = ItemMetadata {
            milestone: state.milestone.clone(),
            lead: state.lead.clone(),
            contributors: state.contributors.clone(),
        };
        let cleaned = extract_inline_metadata(bullet, &mut item_metadata);
        if cleaned.trim().is_empty() {
            continue;
        }
        let (kind, title) = parse_issue_title(&cleaned);
        issues.push(ParsedIssue {
            source_line,
            initiative: state.initiative.clone(),
            project,
            milestone: item_metadata.milestone,
            lead: item_metadata.lead,
            contributors: item_metadata.contributors,
            kind,
            title,
            raw: bullet.to_string(),
        });
    }

    ParsedBacklog {
        issues,
        project_notes,
        warnings,
    }
}

fn apply_heading(state: &mut ParserState, level: usize, heading: &str) {
    if is_milestone_heading(heading) || level >= 4 {
        state.milestone = Some(clean_milestone_heading(heading));
    } else if is_project_heading(heading) || level == 3 {
        state.project = Some(clean_project_heading(heading));
        state.milestone = None;
        state.lead = None;
        state.contributors.clear();
    } else if is_initiative_heading(heading) || level <= 2 {
        state.initiative = Some(clean_initiative_heading(heading));
        state.project = None;
        state.milestone = None;
        state.lead = None;
        state.contributors.clear();
    }
}

fn apply_metadata(
    state: &mut ParserState,
    project_notes: &mut BTreeMap<String, Vec<String>>,
    key: MetadataKey,
    value: String,
) {
    match key {
        MetadataKey::Lead => state.lead = Some(value),
        MetadataKey::Contributors => state.contributors = parse_contributors(&value),
        MetadataKey::Milestone => state.milestone = Some(value),
        MetadataKey::ProjectNote => {
            if let Some(project) = state.project.as_ref() {
                project_notes
                    .entry(project.clone())
                    .or_default()
                    .push(value);
            }
        }
    }
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if level == 0 {
        return None;
    }
    let heading = line.get(level..)?.trim();
    if heading.is_empty() {
        return None;
    }
    Some((level, heading.trim_matches('#').trim().to_string()))
}

fn strip_bullet_marker(line: &str) -> Option<&str> {
    let stripped = match line.chars().next()? {
        '-' | '*' | '+' => line.get(1..)?.trim_start(),
        character if character.is_ascii_digit() => strip_ordered_marker(line)?,
        _ => return None,
    };
    Some(strip_checkbox(stripped))
}

fn strip_ordered_marker(line: &str) -> Option<&str> {
    let dot = line.find('.')?;
    if line
        .get(..dot)?
        .chars()
        .all(|character| character.is_ascii_digit())
    {
        line.get(dot + 1..).map(str::trim_start)
    } else {
        None
    }
}

fn strip_checkbox(value: &str) -> &str {
    let trimmed = value.trim_start();
    for marker in ["[ ]", "[x]", "[X]"] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    trimmed
}

#[derive(Debug, Clone, Copy)]
enum MetadataKey {
    Lead,
    Contributors,
    Milestone,
    ProjectNote,
}

fn parse_metadata_line(line: &str) -> Option<(MetadataKey, String)> {
    let normalized = line.trim().trim_matches('*').trim();
    let (key, value) = normalized.split_once(':')?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    metadata_key(key).map(|metadata_key| (metadata_key, value.to_string()))
}

fn metadata_key(key: &str) -> Option<MetadataKey> {
    match normalize_lookup_key(key).as_str() {
        "lead" | "owner" | "dri" | "assignee" => Some(MetadataKey::Lead),
        "contributors" | "supporting contributors" | "support" => Some(MetadataKey::Contributors),
        "milestone" | "release" => Some(MetadataKey::Milestone),
        "note" | "notes" | "project note" | "project notes" | "cross-cutting notes" => {
            Some(MetadataKey::ProjectNote)
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct ItemMetadata {
    milestone: Option<String>,
    lead: Option<String>,
    contributors: Vec<String>,
}

fn extract_inline_metadata(value: &str, metadata: &mut ItemMetadata) -> String {
    let without_brackets = extract_bracket_metadata(value, metadata);
    let mut kept_segments = Vec::new();
    for segment in without_brackets.split('|') {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, value)) = parse_metadata_line(trimmed) {
            apply_item_metadata(metadata, key, value);
        } else {
            kept_segments.push(trimmed);
        }
    }

    if kept_segments.is_empty() {
        without_brackets.trim().to_string()
    } else {
        kept_segments.join(" | ")
    }
}

fn extract_bracket_metadata(value: &str, metadata: &mut ItemMetadata) -> String {
    let mut output = String::new();
    let mut remaining = value;

    while let Some(start) = remaining.find('[') {
        let before = remaining.get(..start).unwrap_or_default();
        output.push_str(before);
        let Some(after_start) = remaining.get(start + 1..) else {
            output.push_str(remaining.get(start..).unwrap_or_default());
            return output.trim().to_string();
        };
        let Some(end) = after_start.find(']') else {
            output.push_str(remaining.get(start..).unwrap_or_default());
            return output.trim().to_string();
        };
        let tag = after_start.get(..end).unwrap_or_default().trim();
        if is_checkbox_tag(tag) {
            output.push('[');
            output.push_str(tag);
            output.push(']');
        } else if let Some((key, value)) = parse_metadata_line(tag) {
            apply_item_metadata(metadata, key, value);
        } else if is_canonical_milestone(tag) {
            metadata.milestone = Some(tag.to_string());
        } else {
            output.push('[');
            output.push_str(tag);
            output.push(']');
        }
        remaining = after_start.get(end + 1..).unwrap_or_default();
    }

    output.push_str(remaining);
    output.trim().to_string()
}

fn apply_item_metadata(metadata: &mut ItemMetadata, key: MetadataKey, value: String) {
    match key {
        MetadataKey::Lead => metadata.lead = Some(value),
        MetadataKey::Contributors => metadata.contributors = parse_contributors(&value),
        MetadataKey::Milestone => metadata.milestone = Some(value),
        MetadataKey::ProjectNote => {}
    }
}

fn is_checkbox_tag(tag: &str) -> bool {
    matches!(tag.trim(), "" | " " | "x" | "X")
}

fn parse_issue_title(value: &str) -> (String, String) {
    let trimmed = value.trim();
    for (prefix, kind) in [
        ("User story:", "story"),
        ("Story:", "story"),
        ("Task:", "task"),
        ("Bug:", "bug"),
    ] {
        if trimmed
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        {
            let title = trimmed
                .get(prefix.len()..)
                .map(str::trim)
                .unwrap_or(trimmed)
                .to_string();
            return (kind.to_string(), title);
        }
    }

    ("task".to_string(), trimmed.to_string())
}

fn parse_contributors(value: &str) -> Vec<String> {
    value
        .replace(" and ", ",")
        .split([',', ';', '&'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn is_initiative_heading(heading: &str) -> bool {
    normalize_lookup_key(heading).starts_with("initiative")
}

fn is_project_heading(heading: &str) -> bool {
    normalize_lookup_key(heading).starts_with("project")
}

fn is_milestone_heading(heading: &str) -> bool {
    let normalized = normalize_lookup_key(heading);
    normalized.starts_with("milestone") || is_canonical_milestone(&normalized)
}

fn is_canonical_milestone(value: &str) -> bool {
    matches!(
        normalize_lookup_key(value).as_str(),
        "4/17" | "alpha" | "next"
    )
}

fn clean_initiative_heading(heading: &str) -> String {
    clean_labeled_heading(heading, "initiative")
}

fn clean_project_heading(heading: &str) -> String {
    clean_labeled_heading(heading, "project")
}

fn clean_milestone_heading(heading: &str) -> String {
    clean_labeled_heading(heading, "milestone")
}

fn clean_labeled_heading(heading: &str, label: &str) -> String {
    let trimmed = heading.trim();
    let normalized = trimmed.to_ascii_lowercase();
    if !normalized.starts_with(label) {
        return trimmed.to_string();
    }

    let rest = trimmed.get(label.len()..).unwrap_or_default().trim_start();
    for delimiter in [":", "-"] {
        if let Some((_, value)) = rest.split_once(delimiter) {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }

    if rest.is_empty() {
        trimmed.to_string()
    } else {
        rest.to_string()
    }
}

fn render_issue_description(
    item: &ParsedIssue,
    project_notes: Option<&Vec<String>>,
    state: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    lines.push(item.raw.clone());
    lines.push(String::new());
    lines.push("## Cycle 88 Backlog Context".to_string());
    if let Some(initiative) = item.initiative.as_ref() {
        lines.push(format!("- Initiative: {initiative}"));
    }
    lines.push(format!("- Project: {}", item.project));
    if let Some(milestone) = item.milestone.as_ref() {
        lines.push(format!("- Milestone: {milestone}"));
    }
    if let Some(lead) = item.lead.as_ref() {
        lines.push(format!("- Lead: {lead}"));
    }
    if !item.contributors.is_empty() {
        lines.push(format!(
            "- Supporting contributors: {}",
            item.contributors.join(", ")
        ));
    }
    if let Some(state) = state {
        lines.push(format!("- Target state: {state}"));
    }
    lines.push(format!("- Source line: {}", item.source_line));

    if let Some(notes) = project_notes.filter(|notes| !notes.is_empty()) {
        lines.push(String::new());
        lines.push("## Project Notes".to_string());
        for note in notes {
            lines.push(format!("- {note}"));
        }
    }

    lines.join("\n")
}

#[derive(Debug, Serialize)]
struct BacklogIngestReport {
    mode: &'static str,
    source: String,
    issue_count: usize,
    project_count: usize,
    milestone_count: usize,
    lead_count: usize,
    warnings: Vec<String>,
    project_notes: BTreeMap<String, Vec<String>>,
    issues: Vec<ParsedIssue>,
    created: Vec<MachineIssueSummary>,
}

impl BacklogIngestReport {
    fn from_parsed(path: &Path, parsed: &ParsedBacklog, apply: bool) -> Self {
        let projects = parsed
            .issues
            .iter()
            .map(|issue| issue.project.clone())
            .collect::<BTreeSet<_>>();
        let milestones = parsed
            .issues
            .iter()
            .filter_map(|issue| issue.milestone.clone())
            .collect::<BTreeSet<_>>();
        let leads = parsed
            .issues
            .iter()
            .filter_map(|issue| issue.lead.clone())
            .collect::<BTreeSet<_>>();

        Self {
            mode: if apply { "apply" } else { "dry-run" },
            source: path.display().to_string(),
            issue_count: parsed.issues.len(),
            project_count: projects.len(),
            milestone_count: milestones.len(),
            lead_count: leads.len(),
            warnings: parsed.warnings.clone(),
            project_notes: parsed.project_notes.clone(),
            issues: parsed.issues.clone(),
            created: Vec::new(),
        }
    }
}

fn render_ingest_report(report: &BacklogIngestReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Backlog ingest {}: {}", report.mode, report.source));
    lines.push(format!(
        "Parsed {} issue(s) across {} project(s), {} milestone(s), and {} lead(s).",
        report.issue_count, report.project_count, report.milestone_count, report.lead_count
    ));
    if !report.created.is_empty() {
        lines.push(format!("Created {} Linear issue(s).", report.created.len()));
    }
    if !report.warnings.is_empty() {
        lines.push(String::new());
        lines.push("Warnings:".to_string());
        for warning in &report.warnings {
            lines.push(format!("- {warning}"));
        }
    }
    lines.push(String::new());
    lines.push("Issues:".to_string());
    for issue in &report.issues {
        let milestone = issue.milestone.as_deref().unwrap_or("missing milestone");
        let lead = issue.lead.as_deref().unwrap_or("missing lead");
        lines.push(format!(
            "- line {} [{} / {} / {}] {}",
            issue.source_line, issue.project, milestone, lead, issue.title
        ));
    }
    if !report.created.is_empty() {
        lines.push(String::new());
        lines.push("Created:".to_string());
        for issue in &report.created {
            lines.push(format!("- {} {}", issue.identifier, issue.title));
        }
    }
    lines.join("\n")
}

fn normalize_lookup_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::parse_shaped_backlog;

    #[test]
    fn parses_heading_metadata_and_inline_tags() {
        let parsed = parse_shaped_backlog(
            r#"
## Initiative: Core
### Project: Wallet Connect
Lead: Greg
Contributors: Alice, Bob
Project notes: Keep launch scope tight.
#### 4/17
- [ ] Story: Connect wallet [Lead: Jane] [Contributors: Raj] [Alpha]
- Task: Add QA checklist
"#,
        );

        assert_eq!(parsed.issues.len(), 2);
        assert_eq!(parsed.issues[0].initiative.as_deref(), Some("Core"));
        assert_eq!(parsed.issues[0].project, "Wallet Connect");
        assert_eq!(parsed.issues[0].milestone.as_deref(), Some("Alpha"));
        assert_eq!(parsed.issues[0].lead.as_deref(), Some("Jane"));
        assert_eq!(parsed.issues[0].contributors, vec!["Raj"]);
        assert_eq!(parsed.issues[0].title, "Connect wallet");
        assert_eq!(parsed.issues[1].milestone.as_deref(), Some("4/17"));
        assert_eq!(parsed.issues[1].lead.as_deref(), Some("Greg"));
        assert_eq!(
            parsed
                .project_notes
                .get("Wallet Connect")
                .and_then(|notes| notes.first())
                .map(String::as_str),
            Some("Keep launch scope tight.")
        );
    }
}
