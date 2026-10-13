## UAR-4 post-TEST_PASS silent Local CR incident — 2026-10-13
- Actual Mac GSA: `CODE_REVIEW_PASS` and `VERIFICATION_RESULT=TEST_PASS` for `M1/J1` exact change set `037c8481472875894f05ccc04828c1e352c52b0b06690dab5c5cff0ebcd378db`; then no CR output and no prompt. PID 24979 was sleeping (0% CPU), TCP to Ollama 127.0.0.1:11434 ESTABLISHED, `qwen38t:latest` loaded, Tokio waiting in kevent/condvar. User used Ctrl+C, host subsequently cooled. The observations do not prove request-inference activity, incorrect model mapping, or a deadlock.
- Fix on main (source commit `a2694a044d440841c664047c829f643f73fbd5a4`): print `CODE_CR_START` before entering local CR, `CODE_CR_MODEL` after model resolution, `CODE_CR_ROUND` at each round; bound Ollama HTTP header wait and each stream idle wait to 300s with explicit error. No DB/reset or verification bypass. Timeout covers inactivity, not overall maximum model inference duration.
- CI run https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38428541529 was queued at update. Do not claim CI PASS until actual conclusion. On Mac, keep the old GSA process stopped; only copy `src/app.rs`, `src/workflow.rs`, `src/ollama.rs` to QA and resume existing J1 after CI PASS. Resume semantics post-TEST_PASS must be verified explicitly before claiming no re-verification. CR and terminal gates still pending.

## UAR-4 actual Mac acceptance — 2026-10-13
- Human Mac QA `/Users/nam/Gsa-local-UAR3-QA.b5R8Pa` resumed from `DurableExact -> RunRequiredVerification` with code workflow `REVIEW_PASS`, `M1/J1`.
- Exact source change set retained: `037c8481472875894f05ccc04828c1e352c52b0b06690dab5c5cff0ebcd378db`.
- GSA output after sandbox fixes: `CODE_REVIEW_PASS jobpack=J1 change_set=037c8481472875894f05ccc04828c1e352c52b0b06690dab5c5cff0ebcd378db`; `VERIFICATION_RESULT jobpack=J1 change_set=037c8481472875894f05ccc04828c1e352c52b0b06690dab5c5cff0ebcd378db result=TEST_PASS`.
- Mac verification **PASS confirmed by actual GSA output**, not inferred from CI or unsandboxed cargo. No evidence yet of independent code CR PASS or terminal/milestone completion; **UAR-4 remains IN_PROGRESS** pending these gates and restart acceptance. Do not rerun Coder, reset SQLite, or fabricate CR evidence.

## UAR-4 nested macOS sandbox verification — 2026-10-13
- Human local evidence: `cargo check` PASS, `cargo test` dependency compilation PASS, but `tests/process_runner.rs` failed with `sandbox-exec: sandbox_apply: Operation not permitted` exit 71. Root cause: tests invoke `LocalProcessRunner::production()` and create a second `sandbox-exec` beneath verification's first sandbox.
- Implementation on main: macOS verification runner marks verified sandbox descendants with `GSA_VERIFICATION_SANDBOX=1`; macOS integration tests run real child controls in that existing sandbox, never nest another sandbox-exec. Outside existing sandbox the same tests continue to exercise real production runner. Negative outside-workspace write control and allowed /dev/null control both remain mandatory, with a separate outer-sandbox test spawning the controls inside production sandbox.
- Fail-closed requirement: no skipped tests; no global unsandboxed Cargo test; no source/README/SQLite mutation or re-running Coder. Keep J1 REVIEW_PASS exact change_set `037c8481472875894f05ccc04828c1e352c52b0b06690dab5c5cff0ebcd378db`.
- Code CI confirmed SUCCESS for `main@f2a5077f00f5294e5a3b84de63d9d60fb5befe2f`: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38425997125 . Real QA acceptance still required. Source of truth is main; QA test must copy both `src/process_runner.rs` and `tests/process_runner.rs` before resuming required verification. Record real result before any UAR-4 PASS claim.

# GSA Local — Project Resume / Handoff
Updated: 2026-10-13 (Asia/Ho_Chi_Minh)

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

## UAR-4 follow-up checkpoint — 2026-10-13
- Current validated UAR-4 source checkpoint: **main@9c55f374565729c14142aaf5be51ccdfd98e2ef9**. Exact GitHub Actions CI **SUCCESS**: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38338361527.
- Previous source test checkpoint main@a9a11f35eae5ec20cadd6fa64c3cfb703fd91ecf also SUCCESS: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38337593517.
- Shared Coder/Internal Fix/Reviewer loop and terminal classifier regression coverage now includes missing/duplicate/mixed terminal calls, malformed terminal payload, Reviewer write denial, malformed action tool arguments without a mutation, bounded tool-round exhaustion across roles, and Coder/Internal Fix terminal parity.
- **Do not mark UAR-4 PASS yet.** GitHub CI verifies synthetic code tests only. No fresh real Mac Ollama coding execution, independent second-model code review, or full workflow acceptance recorded. Prior UAR-3 acceptance cannot be reused as UAR-4 acceptance.
- **NEXT HUMAN GATE**: run isolated Mac Ollama acceptance against this exact main source (not the original `~/Gsa-local/.gsa/state/gsa.db`), exercising an actual ACTIVE Job Pack with Coder submission -> Reviewer verdict -> optional Internal Fix -> Reviewer PASS and restart/resume. Share terminal output and exact tested commit. Do not abandon/overwrite persistent QA state without explicit Human consent.
- Following the Mac outcome, inspect failures, repair if needed, rerun exact-commit CI, perform independent code review/acceptance and only then consider UAR-4 closure and UAR-5.

## UAR-4 simulation-first continuation — 2026-10-13
- Human Mac observed initial M1/J1 Coder entry and actual model terminal checkpoint rejection: `checklist claim T1#4 is not part of active Job Pack J1`. The model-selection issue was independently resolved in the QA session; this is a real model-generated invalid claim, not a Registry acceptance bug.
- Isolated SQLite simulation tests `uar4_simulate_model_invented_t1_position_four` and `uar4_simulate_resume_review_and_internal_fix` PASS at main@518489f, exact CI https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38381543317.
- Production shared loop now prevalidates Coder/Internal Fix checklist claims against actual active tasks, retains strict Registry validation, and returns bounded tool feedback for invalid claims instead of immediately aborting. Implementation main@d1d3bb4, exact CI SUCCESS https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38388198036.
- Simulated validity/duplicate/foreign claim cases added. These are deterministic tests, NOT real Ollama acceptance and NOT proof of durable mutation-journal recovery if the process dies before checkpoint persistence.
- **NEXT HUMAN GATE:** Mac QA must run fresh UAR-4 source with the original QA SQLite untouched; preserve other QA-local source edits before updating. Inspect actual Coder invalid-claim retry, Reviewer and Internal Fix flows. Avoid re-running prior source mutations unconditionally; capture status and logs first. UAR-4 remains IN_PROGRESS.
- New simulation matrix: `.gsa/UAR4_SIMULATION_MATRIX.md`; unresolved crash-before-checkpoint recovery is tracked there.

## UAR-4 recovery simulation + Human Gate — 2026-10-13
- Simulation-first crash finding confirmed from Mac: old QA M1/J1 remains CODER with coder_attempts=0 and no code checkpoint, while QA README source was changed. Human backed up `README.md` and `gsa.db` to `/Users/nam/Gsa-local-UAR4-recovery-backup`.
- Implemented code mutation write-ahead ledger for Coder/Internal Fix, exact graph and Job Pack binding, final per-path hash verification, reviewer file content restoration, multi-write handling, guarded cleanup after durable code checkpoint, strict wrong-job/stale-hash rejection, and denied project_write to `.gsa/state`.
- Source crash-recovery simulation suite CI PASS on main@026eb29ec98f9bcf51ba4e0ff375cc38d84e3f47: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38392070899.
- Explicit legacy-created-file import simulation on main@48a0fc82412329657ca3637f52c3f7214118a10c CI PASS: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38392450904.
- **NEXT HUMAN GATE**: `.gsa/UAR4_HUMAN_GATE.md` and the guarded executable `scripts/uar4-legacy-qa-recovery.sh` (Bash syntax checked in repository CI). Stop the old Mac QA GSA process gracefully; preserve original SQLite, confirm README exact backup + absent canonical README + state v1/J1/CODER, take consistent SQLite backup, then deliberately create a bound recovery ledger for the old README and copy tested source files. Do not silently assume an old write-ahead ledger exists.
- Only after Human Mac runtime acceptance and independent second-model code review may UAR-4 be marked PASS. UAR-5 remains unstarted.
- No lifecycle mutation, no abandonment/reset of historical graph, and no source branch divergence is authorized by this checkpoint.

## UAR-4 Mac verification discovery finding — 2026-10-13
- Mac actual Coder -> Reviewer returned `CODE_REVIEW_PASS` on J1 change_set `037c8481472875894f05ccc04828c1e352c52b0b06690dab5c5cff0ebcd378db`. `VERIFICATION_RESULT=BLOCKED`; J1 ACTIVE.
- SQL exact reason: `scripts/ci.sh contains unsupported verification command: for script in scripts/ci.sh ...; do`, with empty `commands_json`. This is deterministic verifier discovery, not Coder/Reviewer failure.
- Corrected `src/verification.rs` to fall back to fixed `cargo check` and `cargo test` argv when complex CI script syntax is unsupported and `Cargo.toml` exists. Never execute complex Bash through the runtime; simple recognized CI command list behavior retained. Regression on a complex `scripts/ci.sh` fixture.
- Exact implementation `main@8c68f06853178aaaaa553b2797c1b6284d7c0ec8` CI SUCCESS: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38400827685
- Human next: after gracefully stopping existing QA session, copy only verified new `src/verification.rs` to isolated QA preserving SQLite/README/recovery evidence, restart GSA and trigger existing J1 verification without recoding. Capture real `VERIFICATION_RESULT`, CR/TM gates. Do not mark J1 or UAR-4 PASS before real result.

## UAR-4 macOS sandbox cargo-test remediation — 2026-10-13
- Human QA exact evidence: `cargo check` inside GSA sandbox exited 0; `cargo test` exited 101 while `rustix v1.1.5` build script attempted a subprocess (`Operation not permitted`). The same Cargo test using the GSA-target directory outside sandbox PASSED (unit/integration tests).
- Root cause hypothesis (not yet verified on user's Mac): `rustix` build script executes a `rustc` capability probe with `Stdio::null()`, which can require opening `/dev/null` for writing. Sandbox previously denied all file writes except GSA CI runtime/temp paths.
- Narrow implementation: `src/process_runner.rs` verifier sandbox profile permits write to the literal `/dev/null` only, in addition to scoped runtime/temp paths; network and outside-file writes remain denied. macOS regression tests exercise allowed sink write and denied outside workspace write. No source J1, README, verification DB, or workflow state edited.
- Exact code commit `main@20a40cb4509799344afd326c249b0c67ac365433`, CI SUCCESS: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38422247115
- **NEXT HUMAN GATE:** Gracefully close old QA GSA, update main, copy only `src/process_runner.rs` into isolated QA, restart existing persisted REVIEW_PASS J1, and rerun required verification. Confirm exact J1 change_set `037c8481472875894f05ccc04828c1e352c52b0b06690dab5c5cff0ebcd378db` unchanged, `VERIFICATION_RESULT=TEST_PASS` and subsequent CR/terminal gates. A passing macOS regression is not yet proof that user-local rustix Cargo Test passes.
- On continued `FAIL`: capture only new verification stderr/reason from SQLite (do not repeat Coder or reset DB), determine next denied resource precisely before broadening policy. UAR-4 remains IN_PROGRESS.

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
