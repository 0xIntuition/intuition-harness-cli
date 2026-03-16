# Cron Jobs

Use this directory for repository-local automation jobs managed by `meta cron`.

- One Markdown file per job, such as `nightly.md`
- YAML front matter stores the schedule and command metadata
- Markdown body stores operator notes and future-agent context
- `.runtime/` is created on demand for PID files, logs, and scheduler state
