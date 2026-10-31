# PERF.md - Track D (Traverse)

Track D is our dropped track. Its memory work and gap analysis live in the repository
history and in docs/, so this file records the ladder without duplicating them.

## Board runs

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 1 | Radix heap; atomic query claiming; in-degree-0 early exit; settled bitmap | `run-58030bea7c39` | 0.8373 | 169.7 | 48.60 |
| 2 | Compact per-query label store; graph read in windows, never held whole | `run-6d2b00e` | 0.9103 | 77.2 | 50.11 |

<!-- BEGIN GENERATED measured (tooling/gen_docs.py)
     Everything between this marker and the matching END marker is REPLACED on
     regeneration; run IDs are transcribed by hand from the board UI.
-->
<!-- END GENERATED measured -->