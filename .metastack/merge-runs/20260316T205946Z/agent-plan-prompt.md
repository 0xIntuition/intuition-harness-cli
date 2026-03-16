Plan a one-shot aggregate merge run for `0xIntuition/metastack-cli`.
Base branch: `main`
Aggregate branch: `meta-merge/20260316T205946Z`
Choose an explicit merge order and call out likely conflict hotspots before execution.

Return strict JSON with this shape:
{"merge_order":[101,102],"conflict_hotspots":["config.rs","README.md"],"summary":"why this order is safest"}

Selected pull requests:
- #1 ENG-10063: add advanced agent routing | head=`eng-10063-add-advanced-agent-routing-config-resolver-config-surfaces-and` | base=`main` | author=`kamescg` | url=https://github.com/0xIntuition/metastack-cli/pull/1
  body: ## Summary - add install-scoped advanced agent routing config with validated family and command route keys - wire agent-backed commands through the shared route resolver and expose route-aware runtime config surfaces - add coverage for rout...