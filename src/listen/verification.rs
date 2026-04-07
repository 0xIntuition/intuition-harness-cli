use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::fs::PlanningPaths;

const BATTLE_TEST_INPUT_PREVIEW_LIMIT: usize = 1_200;
const EVIDENCE_PREVIEW_LIMIT: usize = 600;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VerificationStatus {
    #[default]
    Skipped,
    Passed,
    Failed,
}

impl VerificationStatus {
    pub(crate) fn display_label(self) -> &'static str {
        match self {
            Self::Skipped => "Skipped",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationFinding {
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationCriterionResult {
    pub name: String,
    #[serde(default)]
    pub status: VerificationStatus,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<VerificationFinding>,
    #[serde(default)]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationRouteDiagnostics {
    pub route_key: String,
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    pub provider_source: String,
    #[serde(default)]
    pub model_source: Option<String>,
    #[serde(default)]
    pub reasoning_source: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationCodeReviewReport {
    #[serde(default)]
    pub status: VerificationStatus,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub criteria: Vec<VerificationCriterionResult>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationE2eStepReport {
    pub name: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub status: VerificationStatus,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub assertions: Vec<String>,
    #[serde(default)]
    pub stdout_excerpt: Option<String>,
    #[serde(default)]
    pub stderr_excerpt: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationE2eReport {
    #[serde(default)]
    pub status: VerificationStatus,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub recipe_path: Option<String>,
    #[serde(default)]
    pub steps: Vec<VerificationE2eStepReport>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationBattleTestCase {
    pub input_path: String,
    #[serde(default)]
    pub status: VerificationStatus,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationBattleTestReport {
    #[serde(default)]
    pub status: VerificationStatus,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub sampled_count: usize,
    #[serde(default)]
    pub cases: Vec<VerificationBattleTestCase>,
    #[serde(default)]
    pub input_dir: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationSummary {
    #[serde(default)]
    pub status: VerificationStatus,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub criteria_total: usize,
    #[serde(default)]
    pub criteria_failed: usize,
    #[serde(default)]
    pub e2e_status: VerificationStatus,
    #[serde(default)]
    pub battle_test_status: VerificationStatus,
    #[serde(default)]
    pub remediation: Vec<String>,
}

impl VerificationSummary {
    pub(crate) fn compact_label(&self) -> String {
        let mut parts = vec![self.status.display_label().to_string()];
        if self.criteria_total > 0 {
            parts.push(format!(
                "criteria {}/{} failed",
                self.criteria_failed, self.criteria_total
            ));
        }
        parts.push(format!("e2e {}", self.e2e_status.label()));
        parts.push(format!("battle {}", self.battle_test_status.label()));
        parts.join(" | ")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationReport {
    pub version: u8,
    pub issue_identifier: String,
    pub turn_number: u32,
    pub generated_at_epoch_seconds: u64,
    #[serde(default)]
    pub status: VerificationStatus,
    pub summary: String,
    #[serde(default)]
    pub route: Option<VerificationRouteDiagnostics>,
    #[serde(default)]
    pub quality_criteria: Vec<String>,
    #[serde(default)]
    pub code_review: VerificationCodeReviewReport,
    #[serde(default)]
    pub e2e: VerificationE2eReport,
    #[serde(default)]
    pub battle_tests: VerificationBattleTestReport,
    #[serde(default)]
    pub remediation: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl VerificationReport {
    pub(crate) fn summary_snapshot(&self) -> VerificationSummary {
        let criteria_total = self.code_review.criteria.len();
        let criteria_failed = self
            .code_review
            .criteria
            .iter()
            .filter(|criterion| criterion.status == VerificationStatus::Failed)
            .count();
        VerificationSummary {
            status: self.status,
            summary: self.summary.clone(),
            criteria_total,
            criteria_failed,
            e2e_status: self.e2e.status,
            battle_test_status: self.battle_tests.status,
            remediation: self.remediation.clone(),
        }
    }

    pub(crate) fn render_markdown(&self) -> String {
        let mut lines = vec![
            format!("# Verification Report: {}", self.issue_identifier),
            String::new(),
            format!("- Status: {}", self.status.display_label()),
            format!("- Summary: {}", self.summary),
            format!("- Turn: {}", self.turn_number),
        ];
        if let Some(route) = self.route.as_ref() {
            lines.push(format!("- Route: {}", route.route_key));
            lines.push(format!("- Provider: {}", route.provider));
            lines.push(format!(
                "- Model: {}",
                route.model.as_deref().unwrap_or("unset")
            ));
            lines.push(format!(
                "- Reasoning: {}",
                route.reasoning.as_deref().unwrap_or("unset")
            ));
        }
        if !self.quality_criteria.is_empty() {
            lines.extend([String::new(), "## Quality Criteria".to_string()]);
            for criterion in &self.quality_criteria {
                lines.push(format!("- {criterion}"));
            }
        }
        lines.extend([
            String::new(),
            "## Code Review".to_string(),
            format!("- Status: {}", self.code_review.status.display_label()),
            format!("- Summary: {}", self.code_review.summary),
        ]);
        for criterion in &self.code_review.criteria {
            lines.push(format!(
                "- {}: {}",
                criterion.name,
                criterion.status.display_label()
            ));
            if !criterion.summary.trim().is_empty() {
                lines.push(format!("  Summary: {}", criterion.summary));
            }
            for finding in &criterion.findings {
                let location = match (finding.file.as_deref(), finding.line) {
                    (Some(file), Some(line)) => format!("{file}:{line}"),
                    (Some(file), None) => file.to_string(),
                    (None, Some(line)) => format!("line {line}"),
                    (None, None) => "workspace".to_string(),
                };
                lines.push(format!("  Finding: {location} {}", finding.message));
            }
            if let Some(remediation) = criterion.remediation.as_deref() {
                lines.push(format!("  Remediation: {remediation}"));
            }
        }
        lines.extend([
            String::new(),
            "## E2E".to_string(),
            format!("- Status: {}", self.e2e.status.display_label()),
            format!("- Summary: {}", self.e2e.summary),
        ]);
        if let Some(recipe_path) = self.e2e.recipe_path.as_deref() {
            lines.push(format!("- Recipe: {recipe_path}"));
        }
        for step in &self.e2e.steps {
            lines.push(format!("- {}: {}", step.name, step.status.display_label()));
            if !step.command.is_empty() {
                lines.push(format!("  Command: {}", step.command.join(" ")));
            }
            if let Some(code) = step.exit_code {
                lines.push(format!("  Exit code: {code}"));
            }
            for assertion in &step.assertions {
                lines.push(format!("  Assertion: {assertion}"));
            }
            if let Some(stdout) = step.stdout_excerpt.as_deref() {
                lines.push(format!("  Stdout: {stdout}"));
            }
            if let Some(stderr) = step.stderr_excerpt.as_deref() {
                lines.push(format!("  Stderr: {stderr}"));
            }
        }
        lines.extend([
            String::new(),
            "## Battle Tests".to_string(),
            format!("- Status: {}", self.battle_tests.status.display_label()),
            format!("- Summary: {}", self.battle_tests.summary),
            format!("- Sampled inputs: {}", self.battle_tests.sampled_count),
        ]);
        if let Some(input_dir) = self.battle_tests.input_dir.as_deref() {
            lines.push(format!("- Input dir: {input_dir}"));
        }
        for case in &self.battle_tests.cases {
            lines.push(format!(
                "- {}: {}",
                case.input_path,
                case.status.display_label()
            ));
            if !case.summary.trim().is_empty() {
                lines.push(format!("  Summary: {}", case.summary));
            }
            if let Some(remediation) = case.remediation.as_deref() {
                lines.push(format!("  Remediation: {remediation}"));
            }
        }
        if !self.remediation.is_empty() {
            lines.extend([String::new(), "## Remediation".to_string()]);
            for item in &self.remediation {
                lines.push(format!("- {item}"));
            }
        }
        if !self.notes.is_empty() {
            lines.extend([String::new(), "## Notes".to_string()]);
            for note in &self.notes {
                lines.push(format!("- {note}"));
            }
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VerificationRecipeStep {
    pub name: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub expect_exit_code: Option<i32>,
    #[serde(default)]
    pub expect_stdout_contains: Vec<String>,
    #[serde(default)]
    pub expect_stderr_contains: Vec<String>,
    #[serde(default)]
    pub expect_paths_exist: Vec<String>,
    #[serde(default)]
    pub expect_paths_missing: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RouteVerificationRecipe {
    #[serde(default)]
    pub quality_criteria: Vec<String>,
    #[serde(default)]
    pub e2e: Vec<VerificationRecipeStep>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedRouteVerificationRecipe {
    pub path: PathBuf,
    pub recipe: RouteVerificationRecipe,
}

#[derive(Debug, Clone)]
pub(crate) struct BattleTestInput {
    pub relative_path: String,
    pub preview: String,
}

fn is_battle_test_candidate(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            !name.starts_with('.')
                && !matches!(name.to_ascii_lowercase().as_str(), "readme" | "readme.md")
        })
}

pub(crate) fn builtin_quality_criteria() -> Vec<String> {
    vec![
        "The branch matches the Linear ticket deliverables.".to_string(),
        "The changed files are internally consistent and ready for review.".to_string(),
        "The requested validation and safety checks are sufficiently covered.".to_string(),
    ]
}

/// Loads the route-scoped verification recipe for the requested execution route when one exists.
///
/// Returns an error when the recipe path cannot be read or parsed.
pub(crate) fn load_route_verification_recipe(
    workspace_path: &Path,
    route_key: &str,
) -> Result<Option<LoadedRouteVerificationRecipe>> {
    let path = PlanningPaths::new(workspace_path)
        .metastack_dir
        .join("verification")
        .join("recipes")
        .join(format!("{route_key}.yaml"));
    if !path.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    let recipe = serde_yaml::from_str::<RouteVerificationRecipe>(&contents)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    Ok(Some(LoadedRouteVerificationRecipe { path, recipe }))
}

/// Discovers deterministic battle-test inputs under the route-scoped verification input directory.
///
/// Returns an error when the input directory cannot be enumerated or one of the sampled files
/// cannot be read.
pub(crate) fn discover_battle_test_inputs(
    workspace_path: &Path,
    route_key: &str,
    battle_test_count: usize,
) -> Result<(PathBuf, Vec<BattleTestInput>)> {
    let input_dir = PlanningPaths::new(workspace_path)
        .metastack_dir
        .join("verification")
        .join("inputs")
        .join(route_key);
    if battle_test_count == 0 || !input_dir.is_dir() {
        return Ok((input_dir, Vec::new()));
    }

    let mut candidates = WalkDir::new(&input_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_battle_test_candidate(path))
        .collect::<Vec<_>>();
    candidates.sort();

    let mut inputs = Vec::new();
    for path in candidates.into_iter().take(battle_test_count) {
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read `{}`", path.display()))?;
        let preview = truncate_for_battle_test_input(&String::from_utf8_lossy(&bytes));
        let relative_path = path
            .strip_prefix(workspace_path)
            .ok()
            .unwrap_or(path.as_path())
            .display()
            .to_string();
        inputs.push(BattleTestInput {
            relative_path,
            preview,
        });
    }

    Ok((input_dir, inputs))
}

pub(crate) fn truncate_for_evidence(text: &str) -> String {
    truncate_with_ellipsis(text.trim(), EVIDENCE_PREVIEW_LIMIT)
}

pub(crate) fn truncate_for_battle_test_input(text: &str) -> String {
    truncate_with_ellipsis(text.trim(), BATTLE_TEST_INPUT_PREVIEW_LIMIT)
}

fn truncate_with_ellipsis(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let mut truncated = text
            .chars()
            .take(limit.saturating_sub(3))
            .collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BATTLE_TEST_INPUT_PREVIEW_LIMIT, EVIDENCE_PREVIEW_LIMIT, discover_battle_test_inputs,
        truncate_for_battle_test_input, truncate_for_evidence,
    };
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn discover_battle_test_inputs_skips_hidden_and_readme_files() {
        let temp = tempdir().expect("tempdir should build");
        let input_dir = temp
            .path()
            .join(".intuition/verification/inputs/agents.listen");
        fs::create_dir_all(&input_dir).expect("input dir should build");
        fs::write(input_dir.join(".gitkeep"), "").expect("gitkeep should write");
        fs::write(input_dir.join("README.md"), "placeholder").expect("readme should write");
        fs::write(input_dir.join("sample.md"), "battle test sample").expect("sample should write");

        let (root, inputs) = discover_battle_test_inputs(temp.path(), "agents.listen", 5)
            .expect("battle inputs should load");

        assert_eq!(
            root,
            temp.path()
                .join(".intuition/verification/inputs/agents.listen")
        );
        assert_eq!(inputs.len(), 1);
        assert_eq!(
            inputs[0].relative_path,
            ".intuition/verification/inputs/agents.listen/sample.md"
        );
        assert_eq!(inputs[0].preview, "battle test sample");
    }

    #[test]
    fn truncate_helpers_preserve_utf8_boundaries() {
        let evidence = truncate_for_evidence(&"é".repeat(EVIDENCE_PREVIEW_LIMIT + 10));
        assert!(evidence.ends_with("..."));
        assert_eq!(evidence.chars().count(), EVIDENCE_PREVIEW_LIMIT);

        let battle =
            truncate_for_battle_test_input(&"界".repeat(BATTLE_TEST_INPUT_PREVIEW_LIMIT + 5));
        assert!(battle.ends_with("..."));
        assert_eq!(battle.chars().count(), BATTLE_TEST_INPUT_PREVIEW_LIMIT);
    }
}
