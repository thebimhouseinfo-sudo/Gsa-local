# GSA Local — Project Resume / Handoff
Updated: 2026-10-12 (Asia/Ho_Chi_Minh)

## Authority and entry point
- GitHub repository: https://github.com/thebimhouseinfo-sudo/Gsa-local
- **main is the only code source of truth.** Do not reconstruct unmerged legacy branches or treat obsolete runs/commits as current implementation.
- Start next session by reading **this file**, then `CURRENT_EXECUTION_BASELINE.md`, `DETAIL_EXECUTION_PLAN.md`, `IMPLEMENTATION_PLAN.md`, and relevant source/tests. The long baseline contains history; the short status here has precedence for progress recency only. Registry/evidence is authoritative for lifecycle states.
- No unapproved rewriting of persisted task lifecycle. GSA Memory Job J-9067 rev 2 previously remained IN_PROGRESS with task-state drift; no new governed lifecycle closure has been verified here.

## Confirmed milestones (evidence)
1. UAR-1 and UAR-2 have prior run/GOAL_RECHECK PASS evidence indexed in CURRENT_EXECUTION_BASELINE.md.
2. UAR-2B: human Mac Ollama probe PASS at its exact older tested commit, carried into main after source reconciliation; do not rebind the old verification to new commits.
3. **UAR-3: real Mac acceptance PASS for durable planning and handoff to execution**. Isolated QA project `/Users/nam/Gsa-local-UAR3-QA.b5R8Pa`, model `qwen38t:latest`; original `~/Gsa-local/.gsa/state/gsa.db` untouched. Human-only legacy abandonment in QA; Planner source evidence (4), Reviewer, Local CR, process interrupt/restart and resume all observed. JobBuilder's invalid graph was rejected, corrected by bounded repair. `PLAN_APPROVED revision=4`, `EXECUTION_GRAPH_REGISTERED version=1`. Initial REGISTERED→execution router bug repaired on main@0980d72; actual Mac restart printed `Resume: DurableExact -> ActivateInitialMilestone`. **First milestone not yet activated**; this was not a Coder/Tester execution acceptance.
   - macOS CI UAR-3 exact source: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38193649315 (SUCCESS)
4. **UAR-4: IN PROGRESS**. First source checkpoint consolidates bounded Coder / Internal Fix / Reviewer tool-calling loop in `src/workflow.rs` (`run_code_role_tool_loop`), preserving individual role prompts, runtime tool permissions, submission exclusivity and limits. Code commit main@3f6f804, status/baseline update main@01f1bd2; CI for both commits **SUCCESS**:
   - https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38194166443
   - https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38194209699
   This is **not** UAR-4 final verification or a new real Mac Ollama test.

## Next work — resume UAR-4
- Inspect latest `main`, actual coding workflow in `src/workflow.rs`, coding tests in `tests/coding_workflow.rs`, and role contracts in `harnesses/`.
- Continue unifying only genuinely duplicated code-model submission/tool-loop behavior across Coder, Internal Fix and code Reviewer; preserve role-specific tool scopes and Reviewer read-only boundary.
- Add focused regressions for shared-loop behavior: missing terminal submission, multiple terminal/tool mixed calls, malformed args, bounded retry/exhaustion, tool permission denials, and exactly one terminal submission; preserve existing Coder↔Reviewer→Internal Fix transitions.
- Run `scripts/ci.sh` via GitHub Actions against exact new main commit. Do independent code review at a mature checkpoint; request a Mac Ollama test only for behavior CI cannot prove. **Never claim UAR-4 PASS from CI alone.**
- After UAR-4, next tracked segments are UAR-5 (Tester shared runtime), UAR-6 (WorkflowIntentPolicy), UAR-7 (restart/E2E acceptance), subject to current plan and recheck.

## Safety / operational notes
- Original project SQLite DB may contain historical legacy planning. Do not run QA abandonment against `~/Gsa-local/.gsa/state/gsa.db`; prefer isolated scratch copy.
- A live GSA process in the same QA directory can hold the exclusive lease. Check exact process/cwd before graceful termination, never force-delete lease/DB.
- Avoid generating many unmerged branches. Commit accepted work to main, keep exact CI provenance and report blockers honestly.
- Human acceptance was for UAR-3 stage/handoff, **not** full product delivery; no fresh Tester evidence for UAR-4 exists yet.
