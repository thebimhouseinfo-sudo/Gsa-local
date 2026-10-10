# GSA Local — Current Execution Baseline

Reconciled on **2026-10-10** (Asia/Bangkok); updated after UAR-2B governed evidence adoption. Source of truth: [Gsa-local `main`](https://github.com/thebimhouseinfo-sudo/Gsa-local/tree/main). Current approved GSA Memory Job: **J-9067**, revision **2**, lifecycle `IN_PROGRESS`; Reviewer and planning CR both **PASS**. This snapshot is an evidence index, **not** a substitute for a durable lifecycle/Verification record.

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
| **UAR-2B** real Ollama schema probe | Human ran `qwen38t:latest` on Mac against **exact** `6475f1e`: Reviewer **26.063s, repairs=1/2 calls**; JobBuilder **44.015s**; Tester **49.496s**; **1 test PASS in 119.61s** and `UAR2B_PROBE_EXECUTED_PASS`. Evidence in [PR #2](https://github.com/thebimhouseinfo-sudo/Gsa-local/pull/2#issuecomment-6093831675) and exact-commit [CI](https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38024239877). | Old Coder **R-AC9E** and old Tester **R-8548** are **SUPERSEDED**, preserving their old target revisions. New Tester evidence-audit **R-5E13 COMPLETED/PASS**, TEST `01a12454-f80d-7380-ad70-f08c8e38e524`, bound **only to tested PR SHA**. New canonical-source Coder adoption **R-0A80 COMPLETED/PASS**, GOAL_RECHECK `01a12458-0762-7045-9a5c-e199a5fa8e76`, bound to **main@84a103b**. Both results persisted via GSA lifecycle tools and read back. **Code CR and Job task-state reconciliation remain outstanding.** |

**Newly observed verification-gate gap:** a prepare-only V2_STABLE `run_complete` call for Tester `R-8548` offered `COMPLETED/PASS` with **no persisted TEST Verification** and the older target still bound. This result was **not written**, because it would be a false authoritative closure. UAR-2B Coder `R-AC9E` correctly refuses success without exact GOAL_RECHECK. The asymmetry is recorded as **LLC #57** in [WORKFLOW_LESSONS_LEARNED.md](WORKFLOW_LESSONS_LEARNED.md); do not use the permissive Tester transition to bypass evidence checks.


**Evidence provenance boundary:** `R-5E13` independently audited the previously supplied Human-run Mac transcript plus exact-source test definitions, CI and the matching Git tree. It **did not** execute Ollama again. Its TEST PASS is a **bounded historical evidence-audit verdict** for `6475f1e` only; the probe's Tester `VERIFY/BLOCKED` fixture is not a product quality verdict. `R-0A80` checked and adopted already-merged UAR-2B code on canonical `main@84a103b`; this was **not a new Coder implementation**. No old Run was rewritten to point at a newer commit and no false Test Verification was attached to it.

The J-9067 Job record still lists UAR-1/2/2B under `tasks[]` as `READY`. These are **not a faithful task-completion record** for the completed Coder Runs. No supported task-state reconciliation transition was demonstrated. Do not manually patch `tasks[]` or declare J-9067 completed.

## Planned work, not lost implementation

UAR-3 (planning, additive PlanningRunState, SQLite brownfield migration, Human legacy recovery and planning-first resume), UAR-4 (Coder/Reviewer/CR shared runtime), UAR-5 (Tester model loop shared runtime), UAR-6 (WorkflowIntentPolicy) and UAR-7 (restart/E2E acceptance) remain future gated work. The current `PlanningWorkflow::run` still calls `begin_plan_workflow` and still has a plan CR REVISE route through `InternalFix`; `TesterExecution` has its own model/tool loop; Coder text still enters a workflow. These match the approved pending UAR-3/4/5/6 tasks, **not merge-induced code loss**.

The historical J-177F freeze text in the three plan documents is kept only for traceability and no longer serves as an active-work instruction. `Cargo.lock` is absent from the source tree, so deterministic dependency pinning remains a separate hardening task. Do not invent lockfile contents.

## Next governed gate — BEFORE UAR-3

1. **Run/Verification reconciliation: DONE for bounded UAR-1/UAR-2/UAR-2B.** UAR-2B old Runs were terminalized as `SUPERSEDED`, and newly scoped Tester/Coder Runs with real historical-source evidence were terminalized with their own exact-target verification refs. All records were persisted and read back.
2. **CR REVIEW IS STILL REQUIRED** at the declared mature boundary after UAR-1/2/2B **before bulk migration**, per active J-9067 `review_triggers`. Planning Local CR PASS is not code CR PASS. The current GSA control-plane `start_critic_review` endpoint is **Human-invoked only**; do not auto-claim this gate. Review the exact current main revision, corresponding source/tree mapping and above provenance limitations before any UAR-3 mutation.
3. **Job `tasks[]` reconciliation not automated:** UAR-1/2/2B still show `READY` inside J-9067 even though governed completed Run/Verification records now exist. No supported individual Task status updater was available in the current surface; preserve the approved planning revision, rely on explicit Run/evidence refs for accurate operational status, and do not directly patch the Job JSON.
4. **UAR-3 remains pending CR.** Begin the approved additive PlanningRunState/SQLite legacy recovery task only when the mandatory CR/gate and exact live `main` source ref are resolved. Do not alter planned UAR-3 scope to work around a missing verification authority.
