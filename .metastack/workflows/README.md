# Workflow Playbooks

`meta workflows` loads repo-local playbooks from this directory in addition to the built-in workflow set.

Playbooks use Markdown files with YAML front matter:

```md
---
name: release-triage
summary: Investigate a release blocker and propose next actions.
provider: codex
parameters:
  - name: incident
    description: Human-readable incident summary.
    required: true
validation:
  - Confirm impact, scope, and current owner.
instructions: |
  Keep the report concise and action-oriented.
---
Incident summary:
{{incident}}
```

Supported front matter keys:

- `name`: unique workflow identifier used by `meta workflows explain|run`
- `summary`: one-line description shown by `meta workflows list`
- `provider`: default local agent/provider name used for `run`
- `parameters`: input contract with `name`, `description`, optional `required`, and optional `default`
- `validation`: checklist items injected into explain/run output
- `instructions`: optional agent instructions rendered separately from the main prompt
- `linear_issue_parameter`: optional parameter name whose value should be resolved from Linear before prompt rendering

Prompt templates can reference workflow parameters plus shared variables such as:

- `{{repo_root}}`
- `{{effective_instructions}}`
- `{{project_rules}}`
- `{{context_bundle}}`
- `{{repo_map}}`
- `{{validation_steps}}`
- `{{issue_identifier}}`, `{{issue_title}}`, `{{issue_url}}`, `{{issue_state}}`, `{{issue_description}}` when `linear_issue_parameter` is set
