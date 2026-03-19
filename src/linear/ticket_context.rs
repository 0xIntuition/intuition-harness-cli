use std::collections::BTreeSet;
use std::fs;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::fs::{ensure_dir, write_text_file};

use super::{IssueComment, IssueLink, IssueSummary, LinearClient, LinearService};

pub(crate) const SCOPED_DISCUSSION_CHAR_BUDGET: usize = 6_000;
pub(crate) const PERSISTED_DISCUSSION_CHAR_BUDGET: usize = 20_000;
const LOCALIZED_CONTEXT_METADATA_FILE: &str = ".ticket-context.json";
const MANIFEST_PATH: &str = "artifacts/ticket-images.md";
const DISCUSSION_PATH: &str = "context/ticket-discussion.md";

#[derive(Debug, Clone)]
pub(crate) struct LocalizedTicketContext {
    pub(crate) issue: IssueSummary,
    pub(crate) scoped_discussion_markdown: String,
    pub(crate) persisted_discussion_markdown: String,
    image_assets: Vec<TicketImageAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalizedTicketContextMetadata {
    ignored_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct TicketImageAsset {
    alt_text: String,
    original_url: String,
    local_path: String,
    source_label: String,
}

#[derive(Debug, Clone)]
struct ParsedImageReference {
    alt_text: String,
    url: String,
    replacement_range: Range<usize>,
}

#[derive(Debug, Clone, Copy)]
enum ImageSourceKind {
    Description,
    Parent,
    Comment(usize),
}

#[derive(Debug, Clone)]
struct ImageSource {
    kind: ImageSourceKind,
    label: String,
}

#[derive(Debug, Default, Clone)]
struct FilenameState {
    used: BTreeSet<String>,
    fallback_index: usize,
}

/// Rewrite ticket image references to local artifact paths and derive discussion context from
/// parent and comment content for downstream command consumers.
pub(crate) fn localize_ticket_context(issue: &IssueSummary) -> LocalizedTicketContext {
    let mut localized_issue = issue.clone();
    let mut filename_state = FilenameState {
        fallback_index: 1,
        ..FilenameState::default()
    };
    let mut image_assets = Vec::new();

    if let Some(description) = localized_issue.description.as_mut() {
        *description = rewrite_markdown_images(
            description,
            ImageSource {
                kind: ImageSourceKind::Description,
                label: "description".to_string(),
            },
            &mut filename_state,
            &mut image_assets,
        );
    }

    if let Some(parent) = localized_issue.parent.as_mut() {
        localize_parent(parent, &mut filename_state, &mut image_assets);
    }

    for (index, comment) in localized_issue.comments.iter_mut().enumerate() {
        localize_comment(comment, index + 1, &mut filename_state, &mut image_assets);
    }

    let scoped_discussion_markdown =
        build_discussion_context(&localized_issue.comments, SCOPED_DISCUSSION_CHAR_BUDGET);
    let persisted_discussion_markdown =
        build_discussion_context(&localized_issue.comments, PERSISTED_DISCUSSION_CHAR_BUDGET);

    LocalizedTicketContext {
        issue: localized_issue,
        scoped_discussion_markdown,
        persisted_discussion_markdown,
        image_assets,
    }
}

/// Persist localized discussion and image artifacts into a backlog issue directory.
///
/// Per-image download failures are logged and left non-fatal so `meta backlog tech` and
/// `meta backlog sync pull` can complete even when one image fetch fails.
pub(crate) async fn materialize_ticket_context<C>(
    service: &LinearService<C>,
    context: &LocalizedTicketContext,
    issue_dir: &Path,
) -> Result<()>
where
    C: LinearClient,
{
    ensure_dir(issue_dir)?;
    ensure_dir(&issue_dir.join("artifacts"))?;
    ensure_dir(&issue_dir.join("context"))?;

    write_text_file(
        &issue_dir.join(DISCUSSION_PATH),
        &context.persisted_discussion_markdown,
        true,
    )
    .with_context(|| {
        format!(
            "failed to write localized discussion context into `{}`",
            issue_dir.join(DISCUSSION_PATH).display()
        )
    })?;

    write_text_file(
        &issue_dir.join(MANIFEST_PATH),
        &render_ticket_image_manifest(&context.image_assets),
        true,
    )
    .with_context(|| {
        format!(
            "failed to write ticket image manifest into `{}`",
            issue_dir.join(MANIFEST_PATH).display()
        )
    })?;

    let mut ignored_paths = vec![DISCUSSION_PATH.to_string(), MANIFEST_PATH.to_string()];
    for image in &context.image_assets {
        ignored_paths.push(image.local_path.clone());

        let destination = issue_dir.join(&image.local_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }

        match service.download_file(&image.original_url).await {
            Ok(bytes) => {
                fs::write(&destination, bytes)
                    .with_context(|| format!("failed to write `{}`", destination.display()))?;
            }
            Err(error) => {
                eprintln!(
                    "warning: failed to download ticket image `{}` into `{}`: {error:#}",
                    image.original_url, image.local_path
                );
            }
        }
    }

    let metadata_path = issue_dir.join(LOCALIZED_CONTEXT_METADATA_FILE);
    let metadata = serde_json::to_string_pretty(&LocalizedTicketContextMetadata { ignored_paths })
        .context("failed to encode localized ticket context metadata")?;
    write_text_file(&metadata_path, &metadata, true)
        .with_context(|| format!("failed to write `{}`", metadata_path.display()))?;

    Ok(())
}

/// Load generated localized ticket-context paths that should not participate in backlog sync
/// hashing or managed attachment uploads.
pub(crate) fn load_localized_ticket_context_ignored_paths(
    issue_dir: &Path,
) -> Result<BTreeSet<String>> {
    let metadata_path = issue_dir.join(LOCALIZED_CONTEXT_METADATA_FILE);
    if !metadata_path.is_file() {
        return Ok(BTreeSet::new());
    }

    let contents = fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read `{}`", metadata_path.display()))?;
    let metadata: LocalizedTicketContextMetadata = serde_json::from_str(&contents)
        .with_context(|| format!("failed to decode `{}`", metadata_path.display()))?;
    Ok(metadata.ignored_paths.into_iter().collect())
}

fn localize_parent(
    parent: &mut IssueLink,
    filename_state: &mut FilenameState,
    image_assets: &mut Vec<TicketImageAsset>,
) {
    if let Some(description) = parent.description.as_mut() {
        *description = rewrite_markdown_images(
            description,
            ImageSource {
                kind: ImageSourceKind::Parent,
                label: format!("parent {}", parent.identifier),
            },
            filename_state,
            image_assets,
        );
    }
}

fn localize_comment(
    comment: &mut IssueComment,
    comment_number: usize,
    filename_state: &mut FilenameState,
    image_assets: &mut Vec<TicketImageAsset>,
) {
    let label = comment_source_label(comment.body.as_str(), comment_number);
    comment.body = rewrite_markdown_images(
        &comment.body,
        ImageSource {
            kind: ImageSourceKind::Comment(comment_number),
            label,
        },
        filename_state,
        image_assets,
    );
}

fn rewrite_markdown_images(
    markdown: &str,
    source: ImageSource,
    filename_state: &mut FilenameState,
    image_assets: &mut Vec<TicketImageAsset>,
) -> String {
    let image_references = parse_markdown_images(markdown);
    if image_references.is_empty() {
        return markdown.to_string();
    }

    let mut rewritten = String::with_capacity(markdown.len());
    let mut cursor = 0usize;
    for image in image_references {
        rewritten.push_str(&markdown[cursor..image.replacement_range.start]);
        let filename = generate_ticket_image_filename(&image.url, source.kind, filename_state);
        let local_path = format!("artifacts/{filename}");
        rewritten.push_str(&format!("![{}]({local_path})", image.alt_text));
        image_assets.push(TicketImageAsset {
            alt_text: image.alt_text,
            original_url: image.url,
            local_path,
            source_label: source.label.clone(),
        });
        cursor = image.replacement_range.end;
    }
    rewritten.push_str(&markdown[cursor..]);
    rewritten
}

fn parse_markdown_images(markdown: &str) -> Vec<ParsedImageReference> {
    let bytes = markdown.as_bytes();
    let mut cursor = 0usize;
    let mut images = Vec::new();

    while cursor + 4 <= bytes.len() {
        if bytes[cursor] == b'!' && bytes.get(cursor + 1) == Some(&b'[') {
            let Some(alt_end) = find_byte(bytes, cursor + 2, b']') else {
                break;
            };
            if bytes.get(alt_end + 1) != Some(&b'(') {
                cursor += 1;
                continue;
            }
            let Some(url_end) = find_byte(bytes, alt_end + 2, b')') else {
                break;
            };

            let alt_text = markdown[cursor + 2..alt_end].to_string();
            let url = markdown[alt_end + 2..url_end].trim().to_string();
            if !url.is_empty() {
                images.push(ParsedImageReference {
                    alt_text,
                    url,
                    replacement_range: cursor..url_end + 1,
                });
            }
            cursor = url_end + 1;
            continue;
        }

        cursor += 1;
    }

    images
}

fn find_byte(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    bytes
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, byte)| (*byte == needle).then_some(index))
}

fn generate_ticket_image_filename(
    url: &str,
    source_kind: ImageSourceKind,
    state: &mut FilenameState,
) -> String {
    let original_name = Url::parse(url)
        .ok()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let sanitized = sanitize_filename(&original_name);
    let extension = detect_extension(&sanitized).unwrap_or("bin");

    let candidate = match source_kind {
        ImageSourceKind::Description if !sanitized.is_empty() => sanitized,
        ImageSourceKind::Parent if !sanitized.is_empty() => format!("parent-{sanitized}"),
        ImageSourceKind::Comment(index) if !sanitized.is_empty() => {
            format!("comment-{index}-{sanitized}")
        }
        _ => fallback_filename(extension, state),
    };

    dedupe_filename(candidate, state)
}

fn dedupe_filename(candidate: String, state: &mut FilenameState) -> String {
    if state.used.insert(candidate.clone()) {
        return candidate;
    }

    let (stem, extension) = split_stem_and_extension(candidate.as_str());
    let mut suffix = 2usize;
    loop {
        let next = if extension.is_empty() {
            format!("{stem}-{suffix}")
        } else {
            format!("{stem}-{suffix}.{extension}")
        };
        if state.used.insert(next.clone()) {
            return next;
        }
        suffix += 1;
    }
}

fn fallback_filename(extension: &str, state: &mut FilenameState) -> String {
    let filename = format!("image-{}.{}", state.fallback_index, extension);
    state.fallback_index += 1;
    filename
}

fn split_stem_and_extension(filename: &str) -> (&str, &str) {
    match filename.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => (stem, extension),
        _ => (filename, ""),
    }
}

fn detect_extension(filename: &str) -> Option<&str> {
    filename.rsplit_once('.').map(|(_, extension)| extension)
}

fn sanitize_filename(filename: &str) -> String {
    let mut chars = filename.chars().peekable();
    let mut rendered = String::new();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let next_a = chars.peek().copied();
            if next_a.is_some() {
                chars.next();
            }
            let next_b = chars.peek().copied();
            if next_b.is_some() {
                chars.next();
            }
            rendered.push('-');
            continue;
        }

        rendered.push(match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => ch,
            _ => '-',
        });
    }

    rendered
        .trim_matches('.')
        .trim_matches('-')
        .to_ascii_lowercase()
}

fn comment_source_label(body: &str, comment_number: usize) -> String {
    for line in body.lines() {
        let stripped = strip_markdown_formatting(line);
        if !stripped.is_empty() {
            return truncate_chars(&stripped, 80);
        }
    }
    format!("comment-{comment_number}")
}

fn strip_markdown_formatting(line: &str) -> String {
    let mut rendered = String::new();
    let mut chars = line.trim().chars().peekable();

    while matches!(chars.peek(), Some('#' | '>' | '-' | '*' | '+' | ' ' | '\t')) {
        chars.next();
    }

    while let Some(ch) = chars.next() {
        match ch {
            '`' | '*' | '_' | '~' => {}
            '!' if matches!(chars.peek(), Some('[')) => {
                chars.next();
                for next in chars.by_ref() {
                    if next == ']' {
                        break;
                    }
                }
                if matches!(chars.peek(), Some('(')) {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }
                }
            }
            '[' => {
                let mut label = String::new();
                for next in chars.by_ref() {
                    if next == ']' {
                        break;
                    }
                    label.push(next);
                }
                rendered.push_str(&label);
                if matches!(chars.peek(), Some('(')) {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }
                }
            }
            _ => rendered.push(ch),
        }
    }

    rendered.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn build_discussion_context(comments: &[IssueComment], budget: usize) -> String {
    if comments.is_empty() {
        return String::new();
    }

    let mut sections = comments
        .iter()
        .enumerate()
        .map(|(index, comment)| {
            let author = comment.author_name.as_deref().unwrap_or("Unknown");
            let date = comment
                .created_at
                .as_deref()
                .and_then(|value| value.get(..10))
                .unwrap_or("Unknown");
            let rendered = format!("### **{author}** ({date})\n\n{}\n", comment.body.trim());
            (comment.created_at.clone(), index, rendered)
        })
        .collect::<Vec<_>>();
    sections.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

    let mut selected = Vec::new();
    let mut used = 0usize;
    for (_, _, section) in sections.into_iter().rev() {
        let len = section.chars().count();
        if selected.is_empty() && len > budget {
            selected.push(truncate_chars(&section, budget));
            break;
        }
        if used + len > budget {
            continue;
        }
        used += len;
        selected.push(section);
    }
    selected.reverse();
    selected.join("\n")
}

fn render_ticket_image_manifest(images: &[TicketImageAsset]) -> String {
    let mut lines = vec![
        "# Ticket Images".to_string(),
        String::new(),
        "| File | Alt Text | Source | Original URL |".to_string(),
        "| --- | --- | --- | --- |".to_string(),
    ];

    for image in images {
        lines.push(format!(
            "| `{}` | {} | {} | {} |",
            image
                .local_path
                .strip_prefix("artifacts/")
                .unwrap_or(&image.local_path),
            escape_table_cell(image.alt_text.as_str()),
            escape_table_cell(image.source_label.as_str()),
            escape_table_cell(image.original_url.as_str()),
        ));
    }

    lines.join("\n")
}

fn escape_table_cell(value: &str) -> String {
    if value.is_empty() {
        "&nbsp;".to_string()
    } else {
        value.replace('|', "\\|")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FilenameState, ImageSourceKind, build_discussion_context, comment_source_label,
        generate_ticket_image_filename, localize_ticket_context, render_ticket_image_manifest,
    };
    use crate::linear::{IssueComment, IssueLink, IssueSummary, TeamRef, WorkflowState};

    fn issue() -> IssueSummary {
        IssueSummary {
            id: "issue-1".to_string(),
            identifier: "MET-12".to_string(),
            title: "Parent".to_string(),
            description: Some(
                "See ![diagram](https://cdn.example.com/design.png?token=1) for details."
                    .to_string(),
            ),
            url: "https://linear.app/MET-12".to_string(),
            priority: None,
            estimate: None,
            updated_at: "2026-03-18T00:00:00Z".to_string(),
            team: TeamRef {
                id: "team-1".to_string(),
                key: "MET".to_string(),
                name: "Metastack".to_string(),
            },
            project: None,
            assignee: None,
            labels: Vec::new(),
            comments: vec![
                IssueComment {
                    id: "comment-1".to_string(),
                    body: "![skip](https://example.com/skip.png)\n## Triage summary\nNeed follow-up ![shot](https://example.com/comment.png)".to_string(),
                    created_at: Some("2026-03-17T10:00:00Z".to_string()),
                    author_name: Some("Casey".to_string()),
                    resolved_at: None,
                },
                IssueComment {
                    id: "comment-2".to_string(),
                    body: "Later update".to_string(),
                    created_at: Some("2026-03-18T11:00:00Z".to_string()),
                    author_name: Some("Jordan".to_string()),
                    resolved_at: None,
                },
            ],
            state: Some(WorkflowState {
                id: "state-1".to_string(),
                name: "In Progress".to_string(),
                kind: Some("started".to_string()),
            }),
            attachments: Vec::new(),
            parent: Some(IssueLink {
                id: "parent-1".to_string(),
                identifier: "MET-01".to_string(),
                title: "Program".to_string(),
                url: "https://linear.app/MET-01".to_string(),
                description: Some(
                    "Parent image ![parent](https://example.com/parent shot.jpeg)".to_string(),
                ),
            }),
            children: Vec::new(),
        }
    }

    #[test]
    fn localize_ticket_context_rewrites_all_sources_and_builds_discussion() {
        let localized = localize_ticket_context(&issue());

        assert_eq!(
            localized.issue.description.as_deref(),
            Some("See ![diagram](artifacts/design.png) for details.")
        );
        assert_eq!(
            localized
                .issue
                .parent
                .as_ref()
                .and_then(|parent| parent.description.as_deref()),
            Some("Parent image ![parent](artifacts/parent-parent-shot.jpeg)")
        );
        assert!(
            localized.issue.comments[0]
                .body
                .contains("![shot](artifacts/comment-1-comment.png)")
        );
        assert!(
            localized
                .scoped_discussion_markdown
                .contains("### **Casey** (2026-03-17)")
        );
        assert!(
            localized
                .scoped_discussion_markdown
                .contains("### **Jordan** (2026-03-18)")
        );
    }

    #[test]
    fn comment_source_label_prefers_first_meaningful_text_line() {
        assert_eq!(
            comment_source_label(
                "![only](https://example.com/a.png)\n> **Status:** Waiting on rollout",
                3
            ),
            "Status: Waiting on rollout"
        );
        assert_eq!(comment_source_label("   \n![only](x)\n", 4), "comment-4");
    }

    #[test]
    fn filename_generation_preserves_description_basename_and_prefixes_other_sources() {
        let mut state = FilenameState {
            fallback_index: 1,
            ..FilenameState::default()
        };

        assert_eq!(
            generate_ticket_image_filename(
                "https://example.com/My Diagram.png",
                ImageSourceKind::Description,
                &mut state
            ),
            "my-diagram.png"
        );
        assert_eq!(
            generate_ticket_image_filename(
                "https://example.com/My Diagram.png",
                ImageSourceKind::Parent,
                &mut state
            ),
            "parent-my-diagram.png"
        );
        assert_eq!(
            generate_ticket_image_filename(
                "https://example.com/%20",
                ImageSourceKind::Comment(2),
                &mut state
            ),
            "image-1.bin"
        );
    }

    #[test]
    fn manifest_renders_traceability_table() {
        let manifest =
            render_ticket_image_manifest(&localize_ticket_context(&issue()).image_assets);

        assert!(manifest.contains("| `design.png` | diagram | description |"));
        assert!(manifest.contains("| `comment-1-comment.png` | shot | Triage summary |"));
    }

    #[test]
    fn discussion_context_honors_budget_and_keeps_latest_comments() {
        let comments = vec![
            IssueComment {
                id: "comment-1".to_string(),
                body: "first".repeat(400),
                created_at: Some("2026-03-10T00:00:00Z".to_string()),
                author_name: Some("A".to_string()),
                resolved_at: None,
            },
            IssueComment {
                id: "comment-2".to_string(),
                body: "second".repeat(400),
                created_at: Some("2026-03-11T00:00:00Z".to_string()),
                author_name: Some("B".to_string()),
                resolved_at: None,
            },
        ];

        let scoped = build_discussion_context(&comments, 200);
        assert!(scoped.contains("### **B** (2026-03-11)"));
        assert!(!scoped.contains("### **A** (2026-03-10)"));
    }
}
