Plan a one-shot aggregate merge run for `0xIntuition/metastack-cli`.
Base branch: `main`
Aggregate branch: `meta-merge/20260316T230447Z`
Choose an explicit merge order and call out likely conflict hotspots before execution.

Return strict JSON with this shape:
{"merge_order":[101,102],"conflict_hotspots":["config.rs","README.md"],"summary":"why this order is safest"}

Selected pull requests:
- #7 ENG-10095: Update meta agents listen dashboard refresh cadence | head=`eng-10095-update-meta-agents-listen-dashboard-interface` | base=`main` | author=`kamescg` | url=https://github.com/0xIntuition/metastack-cli/pull/7
  body: ## Summary - decouple the live listen dashboard refresh cadence from the Linear poll interval - refresh dashboard session data from persisted listen state every second while keeping Linear polling configurable - add focused coverage for sta...