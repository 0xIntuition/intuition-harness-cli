use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::agents::run_agent_capture;
use crate::cli::{BacklogReviewArgs, RunAgentArgs};
use crate::config::{
    AGENT_ROUTE_BACKLOG_REVIEW, AgentConfigOverrides, AgentConfigSource, AppConfig, PlanningMeta,
    load_required_planning_meta, parse_review_states_csv, resolve_agent_config,
};
use crate::fs::canonicalize_existing_dir;
use crate::linear::{IssueEditSpec, IssueListFilters, IssueSummary};
use crate::{LinearCommandContext, load_linear_command_context};

const REVIEW_LIST_LIMIT: usize = 250;

#[derive(Debug, Clone, Serialize)]
struct ReviewCommandReport {
    project: Option<String>,
    project_id: Option<String>,
    states: Vec<String>,
    reviewed_label: String,
    assessments: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<String>>,
    issues: Vec<ReviewedIssue>,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewedIssue {
    identifier: String,
    title: String,
    url: String,
    classification: ReviewClassification,
    proposed_next_action: ReviewAction,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    proposed_description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    follow_up_questions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    handoff_command: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReviewClassification {
    AlreadyReviewed,
    AlreadyGood,
    SuggestedEdits,
    FollowUpQuestions,
    ReadyForTechnicalScoping,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReviewAction {
    None,
    MarkReviewed,
    ApplySuggestedDescription,
    SurfaceFollowUpQuestions,
    HandoffToTechnicalScoping,
}

#[derive(Debug, Clone)]
struct ReviewExecutionContext {
    reviewed_label: String,
    plan_label: String,
    project: Option<String>,
    project_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ReviewedIssueDraft {
    issue: IssueSummary,
    classification: ReviewClassification,
    proposed_next_action: ReviewAction,
    summary: String,
    proposed_description: Option<String>,
    follow_up_questions: Vec<String>,
    handoff_command: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentReviewAssessment {
    classification: String,
    summary: String,
    rationale: Option<String>,
    proposed_description: Option<String>,
    #[serde(default)]
    follow_up_questions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptDecision {
    Confirm,
    Skip,
    Cancel,
}

/// Review backlog tickets for the active repository project and optionally apply confirmed updates.
///
/// Returns an error when repo setup or Linear auth is missing, the configured project scope cannot
/// be resolved, the review assessment agent is required but unavailable, or confirmed Linear
/// mutations fail.
pub async fn run_backlog_review(args: &BacklogReviewArgs) -> Result<()> {
    let root = canonicalize_existing_dir(&args.client.root)?;
    let app_config = AppConfig::load()?;
    let planning_meta = load_required_planning_meta(&root, "backlog review")?;
    let LinearCommandContext {
        service,
        default_team,
        default_project_id,
    } = load_linear_command_context(&args.client, None)?;

    let states = resolve_review_states(args, &planning_meta, &app_config);
    let reviewed_label = planning_meta.effective_reviewed_label(&app_config);
    let project = args.project.clone();
    let project_id = if project.is_some() {
        None
    } else {
        default_project_id.clone()
    };
    if project.is_none() && project_id.is_none() {
        bail!(
            "`meta backlog review` requires a repo default project or an explicit `--project <NAME>` override"
        );
    }

    let issues = load_review_issues(
        &service,
        default_team,
        project.clone(),
        project_id.clone(),
        &states,
    )
    .await?;
    let context = ReviewExecutionContext {
        reviewed_label: reviewed_label.clone(),
        plan_label: planning_meta.effective_plan_label(&app_config),
        project,
        project_id,
    };
    let (drafts, diagnostics) =
        assess_issues(&root, &app_config, &planning_meta, args, &context, issues)?;

    let report = ReviewCommandReport {
        project: context.project.clone(),
        project_id: context.project_id.clone(),
        states,
        reviewed_label,
        assessments: drafts
            .iter()
            .filter(|draft| {
                matches!(
                    draft.classification,
                    ReviewClassification::AlreadyGood
                        | ReviewClassification::SuggestedEdits
                        | ReviewClassification::FollowUpQuestions
                )
            })
            .count(),
        diagnostics,
        issues: drafts.iter().map(ReviewedIssue::from).collect(),
    };

    let can_prompt = io::stdin().is_terminal() && io::stdout().is_terminal();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .context("failed to encode backlog review report")?
        );
        return Ok(());
    }

    if args.no_interactive || !can_prompt {
        println!("{}", render_text_report(&report));
        return Ok(());
    }

    run_interactive_review_flow(&service, &report, drafts).await
}

async fn load_review_issues<C>(
    service: &crate::linear::LinearService<C>,
    default_team: Option<String>,
    project: Option<String>,
    project_id: Option<String>,
    states: &[String],
) -> Result<Vec<IssueSummary>>
where
    C: crate::linear::LinearClient,
{
    let api_state = (states.len() == 1).then(|| states[0].clone());
    let mut issues = service
        .list_issues(IssueListFilters {
            team: default_team,
            project,
            project_id,
            state: api_state,
            limit: REVIEW_LIST_LIMIT,
            ..IssueListFilters::default()
        })
        .await?;
    issues.retain(|issue| issue_in_states(issue, states));
    Ok(issues)
}

fn assess_issues(
    root: &Path,
    app_config: &AppConfig,
    planning_meta: &PlanningMeta,
    args: &BacklogReviewArgs,
    context: &ReviewExecutionContext,
    issues: Vec<IssueSummary>,
) -> Result<(Vec<ReviewedIssueDraft>, Option<Vec<String>>)> {
    let mut drafts = Vec::with_capacity(issues.len());
    let mut diagnostics = None;

    for issue in issues {
        if has_label(&issue, &context.reviewed_label) {
            drafts.push(ReviewedIssueDraft {
                summary: format!(
                    "Already carries the reviewed label `{}`.",
                    context.reviewed_label
                ),
                proposed_next_action: ReviewAction::None,
                classification: ReviewClassification::AlreadyReviewed,
                proposed_description: None,
                follow_up_questions: Vec::new(),
                handoff_command: None,
                issue,
            });
            continue;
        }

        if has_label(&issue, &context.plan_label) || has_label(&issue, "plan") {
            drafts.push(ReviewedIssueDraft {
                summary: format!(
                    "Carries the plan label and is a candidate for `meta backlog tech {}`.",
                    issue.identifier
                ),
                proposed_next_action: ReviewAction::HandoffToTechnicalScoping,
                classification: ReviewClassification::ReadyForTechnicalScoping,
                proposed_description: None,
                follow_up_questions: Vec::new(),
                handoff_command: Some(format!("meta backlog tech {}", issue.identifier)),
                issue,
            });
            continue;
        }

        if diagnostics.is_none() {
            let resolved = resolve_agent_config(
                app_config,
                planning_meta,
                Some(AGENT_ROUTE_BACKLOG_REVIEW),
                AgentConfigOverrides {
                    provider: args.agent.clone(),
                    model: args.model.clone(),
                    reasoning: args.reasoning.clone(),
                },
            )?;
            diagnostics = Some(vec![
                format!("Resolved provider: {}", resolved.provider),
                format!(
                    "Resolved model: {}",
                    resolved.model.as_deref().unwrap_or("unset")
                ),
                format!(
                    "Resolved reasoning: {}",
                    resolved.reasoning.as_deref().unwrap_or("unset")
                ),
                format!(
                    "Resolved route key: {}",
                    resolved.route_key.as_deref().unwrap_or("unset")
                ),
                format!(
                    "Provider source: {}",
                    format_agent_config_source(&resolved.provider_source)
                ),
                format!(
                    "Model source: {}",
                    resolved
                        .model_source
                        .as_ref()
                        .map(format_agent_config_source)
                        .unwrap_or_else(|| "unset".to_string())
                ),
                format!(
                    "Reasoning source: {}",
                    resolved
                        .reasoning_source
                        .as_ref()
                        .map(format_agent_config_source)
                        .unwrap_or_else(|| "unset".to_string())
                ),
            ]);
        }
        drafts.push(run_agent_assessment(root, args, issue)?);
    }

    Ok((drafts, diagnostics))
}

fn run_agent_assessment(
    root: &Path,
    args: &BacklogReviewArgs,
    issue: IssueSummary,
) -> Result<ReviewedIssueDraft> {
    let prompt = render_review_prompt(root, &issue);
    let output = run_agent_capture(&RunAgentArgs {
        root: Some(root.to_path_buf()),
        route_key: Some(AGENT_ROUTE_BACKLOG_REVIEW.to_string()),
        agent: args.agent.clone(),
        prompt,
        instructions: None,
        model: args.model.clone(),
        reasoning: args.reasoning.clone(),
        transport: None,
        attachments: Vec::new(),
    })
    .with_context(|| {
        "meta backlog review requires a configured local agent to assess tickets that are not already reviewed or plan-ready"
    })?;
    let parsed: AgentReviewAssessment =
        parse_agent_json(&output.stdout, "backlog review assessment")?;
    normalize_agent_assessment(issue, parsed)
}

fn render_review_prompt(root: &Path, issue: &IssueSummary) -> String {
    let description = issue
        .description
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("_No Linear description was provided._");
    format!(
        "You are reviewing one Linear backlog ticket for the active repository.\n\n\
Repository root: `{}`\n\
Issue:\n\
- Identifier: `{}`\n\
- Title: {}\n\
- Team: {}\n\
- Project: {}\n\
- Labels: {}\n\
- URL: {}\n\n\
Current description:\n````md\n{}\n````\n\n\
Return JSON only using this exact shape:\n\
{{\n\
  \"classification\": \"already_good\" | \"suggested_edits\" | \"follow_up_questions\",\n\
  \"summary\": \"one short paragraph\",\n\
  \"rationale\": \"one short paragraph\",\n\
  \"proposed_description\": \"full markdown rewrite when classification is suggested_edits, otherwise omit or null\",\n\
  \"follow_up_questions\": [\"question 1\", \"question 2\"]\n\
}}\n\n\
Rules:\n\
1. Choose `already_good` when the ticket is already implementation-ready for this repository.\n\
2. Choose `suggested_edits` when the ticket should be improved directly before implementation.\n\
3. Choose `follow_up_questions` when required information is missing or ambiguous.\n\
4. Do not propose technical scoping here; that path is handled separately for plan-labeled tickets.\n\
5. Keep follow-up questions concrete and answerable.\n\
6. Keep proposed rewrites scoped to this repository only.",
        root.display(),
        issue.identifier,
        issue.title,
        issue.team.key,
        issue
            .project
            .as_ref()
            .map(|project| project.name.as_str())
            .unwrap_or("none"),
        if issue.labels.is_empty() {
            "none".to_string()
        } else {
            issue
                .labels
                .iter()
                .map(|label| label.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        },
        issue.url,
        description
    )
}

fn normalize_agent_assessment(
    issue: IssueSummary,
    assessment: AgentReviewAssessment,
) -> Result<ReviewedIssueDraft> {
    let summary = assessment.summary.trim().to_string();
    let rationale = assessment.rationale.unwrap_or_default();
    let summary = if summary.is_empty() {
        rationale.trim().to_string()
    } else {
        summary
    };
    let summary = if summary.is_empty() {
        "No review summary provided.".to_string()
    } else {
        summary
    };
    let proposed_description = assessment
        .proposed_description
        .map(|value| value.trim().replace("\r\n", "\n"))
        .filter(|value| !value.is_empty());
    let follow_up_questions = assessment
        .follow_up_questions
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    match assessment.classification.trim() {
        "already_good" => Ok(ReviewedIssueDraft {
            issue,
            classification: ReviewClassification::AlreadyGood,
            proposed_next_action: ReviewAction::MarkReviewed,
            summary,
            proposed_description: None,
            follow_up_questions: Vec::new(),
            handoff_command: None,
        }),
        "suggested_edits" => Ok(ReviewedIssueDraft {
            issue,
            classification: ReviewClassification::SuggestedEdits,
            proposed_next_action: ReviewAction::ApplySuggestedDescription,
            summary,
            proposed_description: Some(proposed_description.ok_or_else(|| {
                anyhow!(
                    "review assessment returned `suggested_edits` without a proposed description"
                )
            })?),
            follow_up_questions: Vec::new(),
            handoff_command: None,
        }),
        "follow_up_questions" => {
            if follow_up_questions.is_empty() {
                bail!("review assessment returned `follow_up_questions` without any questions");
            }
            Ok(ReviewedIssueDraft {
                issue,
                classification: ReviewClassification::FollowUpQuestions,
                proposed_next_action: ReviewAction::SurfaceFollowUpQuestions,
                summary,
                proposed_description: None,
                follow_up_questions,
                handoff_command: None,
            })
        }
        other => bail!("unsupported backlog review classification `{other}`"),
    }
}

async fn run_interactive_review_flow<C>(
    service: &crate::linear::LinearService<C>,
    report: &ReviewCommandReport,
    drafts: Vec<ReviewedIssueDraft>,
) -> Result<()>
where
    C: crate::linear::LinearClient,
{
    println!("{}", render_text_report(report));
    let mut reader = io::stdin().lock();
    let mut writer = io::stdout().lock();
    let mut applied = 0usize;

    for draft in drafts {
        if draft.classification == ReviewClassification::AlreadyReviewed {
            continue;
        }
        render_issue_review(&mut writer, &draft)?;
        match prompt_default_decision(&draft, &mut reader, &mut writer)? {
            PromptDecision::Skip => continue,
            PromptDecision::Cancel => {
                writeln!(
                    writer,
                    "Review cancelled. Confirmed updates already applied: {applied}."
                )?;
                return Ok(());
            }
            PromptDecision::Confirm => {
                if apply_confirmed_action(service, &draft, &report.reviewed_label).await? {
                    applied += 1;
                }
            }
        }
    }

    writeln!(
        writer,
        "Review complete. Confirmed updates applied: {applied}."
    )?;
    Ok(())
}

fn render_issue_review(writer: &mut impl Write, draft: &ReviewedIssueDraft) -> Result<()> {
    writeln!(writer, "\n== {} ==", draft.issue.identifier)?;
    writeln!(writer, "{}\n{}", draft.issue.title, draft.summary)?;
    writeln!(
        writer,
        "Classification: {}",
        classification_label(draft.classification)
    )?;
    writeln!(
        writer,
        "Default action: {}",
        action_label(draft.proposed_next_action)
    )?;
    if let Some(description) = draft.proposed_description.as_deref() {
        writeln!(writer, "\nProposed description:\n{description}")?;
    }
    if !draft.follow_up_questions.is_empty() {
        writeln!(writer, "\nFollow-up questions:")?;
        for question in &draft.follow_up_questions {
            writeln!(writer, "- {question}")?;
        }
    }
    if let Some(command) = draft.handoff_command.as_deref() {
        writeln!(writer, "\nTechnical handoff: {command}")?;
    }
    Ok(())
}

fn prompt_default_decision(
    draft: &ReviewedIssueDraft,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<PromptDecision> {
    writeln!(
        writer,
        "Confirm default action for {}? [y]es / [s]kip / [q]cancel",
        draft.issue.identifier
    )?;
    writer.flush()?;
    let mut input = String::new();
    reader.read_line(&mut input)?;
    match input.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(PromptDecision::Confirm),
        "q" | "quit" | "cancel" => Ok(PromptDecision::Cancel),
        _ => Ok(PromptDecision::Skip),
    }
}

async fn apply_confirmed_action<C>(
    service: &crate::linear::LinearService<C>,
    draft: &ReviewedIssueDraft,
    reviewed_label: &str,
) -> Result<bool>
where
    C: crate::linear::LinearClient,
{
    match draft.proposed_next_action {
        ReviewAction::None | ReviewAction::SurfaceFollowUpQuestions => Ok(false),
        ReviewAction::MarkReviewed
        | ReviewAction::ApplySuggestedDescription
        | ReviewAction::HandoffToTechnicalScoping => {
            let mut labels = draft
                .issue
                .labels
                .iter()
                .map(|label| label.name.clone())
                .collect::<Vec<_>>();
            if !labels
                .iter()
                .any(|label| label.eq_ignore_ascii_case(reviewed_label))
            {
                labels.push(reviewed_label.to_string());
            }
            service
                .edit_issue(IssueEditSpec {
                    identifier: draft.issue.identifier.clone(),
                    title: None,
                    description: draft.proposed_description.clone(),
                    project: None,
                    state: None,
                    priority: None,
                    labels: Some(dedupe_labels(labels)),
                })
                .await?;
            Ok(true)
        }
    }
}

fn render_text_report(report: &ReviewCommandReport) -> String {
    if report.issues.is_empty() {
        return format!(
            "Reviewed 0 issues for states [{}].",
            report.states.join(", ")
        );
    }

    let mut lines = vec![format!(
        "Reviewed {} issue(s) for states [{}].",
        report.issues.len(),
        report.states.join(", ")
    )];
    if let Some(project) = report.project.as_deref() {
        lines.push(format!("Project override: {project}"));
    } else if let Some(project_id) = report.project_id.as_deref() {
        lines.push(format!("Project ID: {project_id}"));
    }
    lines.push(format!("Reviewed label: {}", report.reviewed_label));
    if let Some(diagnostics) = report.diagnostics.as_ref() {
        lines.extend(diagnostics.iter().cloned());
    }
    lines.push(String::new());
    for issue in &report.issues {
        lines.push(format!(
            "- {} [{}] {} -> {}",
            issue.identifier,
            classification_label(issue.classification),
            issue.title,
            action_label(issue.proposed_next_action)
        ));
    }
    lines.join("\n")
}

fn issue_in_states(issue: &IssueSummary, states: &[String]) -> bool {
    issue
        .state
        .as_ref()
        .map(|state| {
            states
                .iter()
                .any(|candidate| state.name.eq_ignore_ascii_case(candidate))
        })
        .unwrap_or(false)
}

fn resolve_review_states(
    args: &BacklogReviewArgs,
    planning_meta: &PlanningMeta,
    app_config: &AppConfig,
) -> Vec<String> {
    let cli_states = args
        .states
        .iter()
        .flat_map(|value| parse_review_states_csv(value).unwrap_or_else(|| vec![value.clone()]))
        .collect::<Vec<_>>();
    if cli_states.is_empty() {
        planning_meta.effective_review_states(app_config)
    } else {
        cli_states
    }
}

fn has_label(issue: &IssueSummary, expected: &str) -> bool {
    issue
        .labels
        .iter()
        .any(|label| label.name.eq_ignore_ascii_case(expected))
}

fn dedupe_labels(labels: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for label in labels {
        if !deduped
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&label))
        {
            deduped.push(label);
        }
    }
    deduped
}

fn action_label(action: ReviewAction) -> &'static str {
    match action {
        ReviewAction::None => "leave unchanged",
        ReviewAction::MarkReviewed => "mark reviewed",
        ReviewAction::ApplySuggestedDescription => "apply suggested description + mark reviewed",
        ReviewAction::SurfaceFollowUpQuestions => "surface follow-up questions",
        ReviewAction::HandoffToTechnicalScoping => {
            "hand off to `meta backlog tech` and mark reviewed"
        }
    }
}

fn classification_label(classification: ReviewClassification) -> &'static str {
    match classification {
        ReviewClassification::AlreadyReviewed => "already_reviewed",
        ReviewClassification::AlreadyGood => "already_good",
        ReviewClassification::SuggestedEdits => "suggested_edits",
        ReviewClassification::FollowUpQuestions => "follow_up_questions",
        ReviewClassification::ReadyForTechnicalScoping => "ready_for_technical_scoping",
    }
}

fn format_agent_config_source(source: &AgentConfigSource) -> String {
    match source {
        AgentConfigSource::ExplicitOverride => "explicit_override".to_string(),
        AgentConfigSource::CommandRoute(route) => format!("command_route:{route}"),
        AgentConfigSource::FamilyRoute(route) => format!("family_route:{route}"),
        AgentConfigSource::RepoDefault => "repo_default".to_string(),
        AgentConfigSource::GlobalDefault => "global_default".to_string(),
    }
}

fn parse_agent_json<T>(raw: &str, phase: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let trimmed = raw.trim();
    let json = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .or_else(|| {
            trimmed
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
                .map(str::trim)
        })
        .unwrap_or(trimmed);
    serde_json::from_str(json).with_context(|| format!("failed to parse {phase} response as JSON"))
}

impl From<&ReviewedIssueDraft> for ReviewedIssue {
    fn from(value: &ReviewedIssueDraft) -> Self {
        Self {
            identifier: value.issue.identifier.clone(),
            title: value.issue.title.clone(),
            url: value.issue.url.clone(),
            classification: value.classification,
            proposed_next_action: value.proposed_next_action,
            summary: value.summary.clone(),
            proposed_description: value.proposed_description.clone(),
            follow_up_questions: value.follow_up_questions.clone(),
            handoff_command: value.handoff_command.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear::{LabelRef, TeamRef};

    fn sample_issue() -> IssueSummary {
        IssueSummary {
            id: "issue-1".to_string(),
            identifier: "ENG-1".to_string(),
            title: "Sample".to_string(),
            description: Some("Description".to_string()),
            url: "https://linear.app/eng/issue/ENG-1".to_string(),
            priority: None,
            estimate: None,
            updated_at: "2026-03-20T00:00:00Z".to_string(),
            team: TeamRef {
                id: "team-1".to_string(),
                key: "ENG".to_string(),
                name: "Engineering".to_string(),
            },
            project: None,
            assignee: None,
            labels: Vec::new(),
            comments: Vec::new(),
            state: None,
            attachments: Vec::new(),
            parent: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn normalize_agent_assessment_requires_rewrite_for_suggested_edits() {
        let error = normalize_agent_assessment(
            sample_issue(),
            AgentReviewAssessment {
                classification: "suggested_edits".to_string(),
                summary: "Needs work".to_string(),
                rationale: None,
                proposed_description: None,
                follow_up_questions: Vec::new(),
            },
        )
        .expect_err("missing rewrite should fail");
        assert!(error.to_string().contains("without a proposed description"));
    }

    #[test]
    fn prompt_decision_defaults_to_skip() {
        let draft = ReviewedIssueDraft {
            issue: sample_issue(),
            classification: ReviewClassification::AlreadyGood,
            proposed_next_action: ReviewAction::MarkReviewed,
            summary: "Good".to_string(),
            proposed_description: None,
            follow_up_questions: Vec::new(),
            handoff_command: None,
        };
        let mut input = "maybe\n".as_bytes();
        let mut output = Vec::new();
        let decision = prompt_default_decision(&draft, &mut input, &mut output)
            .expect("prompt should succeed");
        assert_eq!(decision, PromptDecision::Skip);
    }

    #[test]
    fn dedupe_labels_preserves_first_spelling() {
        assert_eq!(
            dedupe_labels(vec![
                "Reviewed".to_string(),
                "reviewed".to_string(),
                "plan".to_string()
            ]),
            vec!["Reviewed".to_string(), "plan".to_string()]
        );
    }

    #[test]
    fn has_label_checks_case_insensitively() {
        let mut issue = sample_issue();
        issue.labels.push(LabelRef {
            id: "label-1".to_string(),
            name: "Plan".to_string(),
        });
        assert!(has_label(&issue, "plan"));
    }
}
