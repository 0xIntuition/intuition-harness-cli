# Create Command-Family Bootstraps and Shared App Contexts

Command setup is repeated across `src/lib.rs`, `src/cli.rs`, and multiple command implementations, including root resolution, planning-layout checks, config loading, Linear client creation, and path wiring. Introduce command-family modules and typed app contexts for repo, agent, and Linear dependencies so adding a feature no longer requires touching several global files.

## Acceptance Criteria

- CLI definitions and dispatch are reorganized by command family with a clearer directory structure and ownership boundaries.
- Repeated bootstrap logic for repo roots, planning layout, config precedence, and Linear client construction is replaced by shared typed builders or context objects.
- Contributor-facing guidance documents where new commands and supporting code should be added, reducing the need to edit central catch-all modules for routine feature work.