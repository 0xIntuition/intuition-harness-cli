# Adopt the canonical backlog `_TEMPLATE` across setup, conflict handling, generation, and docs

Replace the current two-file backlog bootstrap with the Markdown tree from `tmp/_TEMPLATE` and make the setup/scaffold flow create that exact structure under `.metastack/backlog/_TEMPLATE` in every repository. Implement explicit conflict handling when setup encounters existing backlog template files, then extend downstream backlog generation so `meta plan` and `meta technical` work with the expanded nested template and its placeholder set. Finish by updating user-facing docs, template references, and automated coverage so the new template is the canonical default and repeat-run behavior stays deterministic.

## Acceptance Criteria

- Fresh setup creates every Markdown file and subdirectory from the canonical template under `.metastack/backlog/_TEMPLATE`.
- Seeded template files are copied as-is from the embedded canonical source before any later issue-specific rendering.
- All scaffold entrypoints that ensure backlog templates use the same canonical template source.
- Interactive setup prompts when canonical template files would overwrite existing backlog files.
- The user can choose overwrite, skip, or cancel, and file writes match the selected action.
- Non-interactive setup paths do not silently overwrite existing backlog template files and return a clear next step.
- Generated backlog items resolve the canonical placeholder set, including `{{BACKLOG_TITLE}}`, `{{BACKLOG_SLUG}}`, and `{{TODAY}}`.
- `index.md` remains required and populated for both planning and technical generation while the rest of the template tree is copied into the issue directory.
- Successful backlog generation does not leave unresolved canonical placeholders in the written files.
- User-facing docs describe the new `.metastack/backlog/_TEMPLATE` structure and setup behavior when files already exist.
- Automated tests cover fresh setup, existing-template conflict handling, and plan/technical generation from the canonical template.
- Internal links and file references in the shipped canonical template either resolve within the repo-managed template tree or are explicitly documented as optional.