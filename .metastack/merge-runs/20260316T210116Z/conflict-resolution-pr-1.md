Resolved the merge conflict in [src/config.rs](/Users/metasudo/workspace/intution/stack/metastack-cli-workspace/merge-runs/20260316T210116Z/src/config.rs) by keeping both sides of the change:

- Preserved repo-scoped Linear auth helpers: `repo_linear_api_key`, `set_repo_linear_api_key`, and the `root`-aware `LinearConfig::from_sources` / `ensure_saved_issue_labels` flow.
- Preserved advanced agent routing support: config validation, route upsert/clear helpers, and route-aware agent resolution.

Validation:
- `cargo test --quiet config:: --lib`
- `cargo test --quiet --test config`

The file is staged, there are no remaining unmerged paths, and the repo is ready for `git commit --no-edit`.
