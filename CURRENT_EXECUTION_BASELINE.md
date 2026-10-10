# GSA Local — Current Execution Baseline

Reconciled on **2026-10-10** (Asia/Bangkok). Source of truth: [Gsa-local `main`](https://github.com/thebimhouseinfo-sudo/Gsa-local/tree/main). Current approved GSA Memory Job: **J-9067**, revision **2**, lifecycle `IN_PROGRESS`; Reviewer and planning CR both **PASS**. This snapshot is an evidence index, **not** a substitute for a durable lifecycle/Verification record.

## Source reconciliation

- PR [#2](https://github.com/thebimhouseinfo-sudo/Gsa-local/pull/2) was **squash merged** into `main` at `75cf522dbfeefebbf92e8065c94e8c4183728a7a`.
- The squash commit and Mac-tested PR commit `6475f1e88aefd2c05d41da1ee63993378f90096b` have the **same Git tree** `6b114bc0d8c92f54e2121abeb706adbd024a2d21`. Squash commits have different ancestry; do **not** claim the tested commit is an ancestor of `main`. This equality applies only to the named commits, not later documentation commits.
- [CI on merged `main`](https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38024835938) passed at `75cf522d`.
- `migrated-plan` and `upgrade-v3` had zero commits ahead of `main`; PR #2 is merged/closed. Historical branches are not alternate source truths.

## UAR checkpoint / GSA durable-state reconciliation

| Item | Actual evidence | Durable disposition |
|---|---|---|
| **UAR-1** runtime, model resolver, config, telemetry | Exact `GOAL_RECHECK/PASS` record `01a10f3a-4feb-7118-af73-3dedf6412725` at `f23f1768` | Coder Run **R-8BAE COMPLETED/PASS** through governed `run_complete`, persist and read-back |
| **UAR-2** typed terminal schema | Exact `GOAL_RECHECK/PASS` record `01a114ce-99fa-7466-a364-1434fabd2efa` at `a1b96538` | Coder Run **R-A97C COMPLETED/PASS** through governed `run_complete`, persist and read-back |
| **UAR-2B** real Ollama schema probe | User ran `qwen38t:latest` on Mac against `6475f1e`: Reviewer **26.063s, repairs=1, invocations=2**; JobBuilder **44.015s**; Tester **49.496s**; 1 test passed in **119.61s** with exact `UAR2B_PROBE_EXECUTED_PASS` marker. Transcript recorded in [PR #2 discussion](https://github.com/thebimhouseinfo-sudo/Gsa-local/pull/2). | **Real probe observed PASS; durable closure NOT complete.** Coder Run **R-AC9E** and Tester Run **R-8548** remain `IN_PROGRESS`, with older immutable input target revisions. Do not rewrite their targets or forge a Tester Verification. |

**Newly observed verification-gate gap:** a prepare-only V2_STABLE `run_complete` call for Tester `R-8548` offered `COMPLETED/PASS` with **no persisted TEST Verification** and the older target still bound. This result was **not written**, because it would be a false authoritative closure. UAR-2B Coder `R-AC9E` correctly refuses success without exact GOAL_RECHECK. The asymmetry is recorded as **LLC #57** in [WORKFLOW_LESSONS_LEARNED.md](WORKFLOW_LESSONS_LEARNED.md); do not use the permissive Tester transition to bypass evidence checks.

The J-9067 Job record still lists UAR-1/2/2B under `tasks[]` as `READY`. These are **not a faithful task-completion record** for the completed Coder Runs. No supported task-state reconciliation transition was demonstrated. Do not manually patch `tasks[]` or declare J-9067 completed.

## Planned work, not lost implementation

UAR-3 (planning, additive PlanningRunState, SQLite brownfield migration, Human legacy recovery and planning-first resume), UAR-4 (Coder/Reviewer/CR shared runtime), UAR-5 (Tester model loop shared runtime), UAR-6 (WorkflowIntentPolicy) and UAR-7 (restart/E2E acceptance) remain future gated work. The current `PlanningWorkflow::run` still calls `begin_plan_workflow` and still has a plan CR REVISE route through `InternalFix`; `TesterExecution` has its own model/tool loop; Coder text still enters a workflow. These match the approved pending UAR-3/4/5/6 tasks, **not merge-induced code loss**.

The historical J-177F freeze text in the three plan documents is kept only for traceability and no longer serves as an active-work instruction. `Cargo.lock` is absent from the source tree, so deterministic dependency pinning remains a separate hardening task. Do not invent lockfile contents.

## Next governed gate — BEFORE UAR-3

1. Reconcile UAR-2B Coder/Tester Run identity with the observed installed-model PASS, **without converting tree equivalence into fabricated exact-target test evidence**.
2. Finish the required independent Tester/CR lifecycle through supported GSA operations, or record the precise gate blocker / explicit Human-bounded recovery decision. Do not auto-close old Run state.
3. Then begin UAR-3 from the exact current `main` HEAD, using approved J-9067 revision 2 and preserving existing code safety/test gates.
