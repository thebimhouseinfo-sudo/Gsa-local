# GSA Local — Active Task List

This task list is the execution baseline after the Tester architecture rebase.

## Status rule

- Completed earlier phases keep their validated core invariants.
- Tester rebase work is additive or contract-changing unless a task explicitly says a previous assumption must be removed.
- Do not start implementation from this list until the current Tester architecture Job is the approved execution target.
- Do not open the next feature phase after Tester work until this impact list is reconciled.

## A. Cross-phase rebaseline

### A1 — Phase 2 Harness contracts
- [ ] Update Planner contract: identify unknown runtime facts/evidence needs; do not guess them.
- [ ] Update Job Builder contract: place Test Checkpoints and create named evidence dependencies.
- [ ] Update Tester contract: PLAN/AUTHOR/EXECUTE/MEASURE/PROBE/RETEST with product source read-only.
- [ ] Update Coder contract: required OBSERVED evidence is authoritative; missing required evidence blocks instead of guessed replacement.
- [ ] Add OBSERVED / IMPLICATION / UNRESOLVED provenance rules.

Gate:
- Harness tests show role responsibilities do not overlap incorrectly.
- Reviewer confirms Tester is not treated as another Reviewer.

### A2 — Phase 3 Registry extensions
- [ ] Add persisted TestCheckpoint definitions.
- [ ] Add checkpoint runtime state: DUE/RUNNING/SATISFIED/BLOCKED/NEEDS_HUMAN.
- [ ] Add Tester attempt identity and exact reviewed target binding.
- [ ] Add structured ExperimentContext samples.
- [ ] Add named evidence outputs + provenance + limitations.
- [ ] Add EvidenceRequirement dependency records.
- [ ] Preserve atomic transitions, stale-write rejection and single-active-work invariants.

Gate:
- old graph/state remains readable through explicit migration/default rules;
- stale evidence cannot satisfy a newer target.

### A3 — Phase 4 checkpoint/resume extension
- [ ] Extend latest-checkpoint stage vocabulary for Tester checkpoint states.
- [ ] Resume exact Tester checkpoint/attempt safely after restart.
- [ ] Preserve one-project execution lease behavior.

Gate:
- restart cannot skip or duplicate a due Tester checkpoint.

### A4 — Phase 5 Planner evidence planning
- [ ] Add first-class `EvidenceNeed / UnknownRuntimeFact` to PlanArtifact.
- [ ] Legacy PlanArtifact JSON without EvidenceNeed must deserialize with `evidence_needs=[]`.
- [ ] Extend `submit_plan` schema and validation for evidence-needs ids, purpose, required/optional flag, intended consumer, expected mode and acceptance/measurement intent.
- [ ] Include EvidenceNeed in plan hash/revision so Reviewer/CR approve the empirical contract itself.
- [ ] Planner may resolve a need from prior compatible OBSERVED evidence only when EvidenceApplicability holds.
- [ ] Unresolved required runtime facts remain explicit; never replace them with guessed values.
- [ ] Job Builder receives EvidenceNeed directly from the approved plan.

Gate:
- Planner still does not create Job Packs/Milestones itself;
- evidence needs survive Planner → Reviewer/CR → PLAN_APPROVED → Job Builder without prose re-inference.

## B. Job Builder and execution graph — major impact

### B1 — TestCheckpointSpec
- [ ] Add explicit checkpoint id.
- [ ] Add explicit boundary kind: AFTER_JOBPACK_SET / BEFORE_JOBPACK / MILESTONE_GATE or equivalent.
- [ ] Add prerequisite Job Pack set + explicit PrerequisiteState.
- [ ] Phase 10 may schedule from exact REVIEW_PASS targets and may read existing DONE, but must never create DONE/COMPLETE.
- [ ] Allow one checkpoint to depend on multiple Job Packs/integrated milestone state.
- [ ] Add modes: VERIFY / MEASURE / PROBE.
- [ ] Add goal and acceptance/measurement criteria.
- [ ] Add required capabilities.
- [ ] Add experiment dimensions when measurements/probes are needed.
- [ ] Add named evidence outputs.
- [ ] Add required/optional output semantics.

### B2 — EvidenceRequirement
- [ ] Later Job Pack can reference a named checkpoint output.
- [ ] Requirement specifies acceptable provenance: OBSERVED only for measured inputs.
- [ ] Requirement records target/applicability compatibility rules.
- [ ] Missing/stale/incompatible evidence blocks consumer activation.

### B3 — Job Builder generation path
- [ ] Extend `submit_execution_graph` tool schema.
- [ ] Extend ExecutionGraph serialization/validation.
- [ ] Extend Registry graph registration.
- [ ] Extend Job Builder harness.
- [ ] Extend Planner→Job Builder contract so evidence needs survive decomposition.
- [ ] Return PLAN_GAP rather than inventing a checkpoint, variable or threshold.

Gate:
- a generated graph can round-trip checkpoint and evidence dependency fields;
- Reviewer PASS alone never creates a Tester checkpoint.

## C. Phase 7 Milestone / Job Pack Controller impact

### C1 — checkpoint boundary resolution
- [ ] Detect when active execution reaches a declared checkpoint boundary.
- [ ] Evaluate every prerequisite Job Pack in the checkpoint prerequisite set.
- [ ] Support single-slice, multi-slice and milestone-gate checkpoints.
- [ ] Do not infer checkpoint from generic Reviewer PASS.
- [ ] Stop progression when a required checkpoint becomes DUE.
- [ ] Return control after SATISFIED without marking Job Pack/Milestone DONE.

### C2 — downstream evidence resolution
- [ ] Resolve EvidenceRequirement before activating consuming work.
- [ ] Evaluate EvidenceApplicability against current product/config/runtime/environment dimensions.
- [ ] Inject exact OBSERVED evidence refs/values into ActiveWork.
- [ ] Block activation on missing/stale/incompatible evidence.

Gate:
- controller cannot jump past a required checkpoint or evidence dependency.

## D. Phase 8 Coder ↔ Reviewer impact

### D1 — preserve coding loop
- [ ] Keep Coder → lightweight self-check → Reviewer ↔ Internal Fix unchanged.
- [ ] Tester remains outside the coding review loop.

### D2 — evidence-aware Coder
- [ ] ActiveWork packet includes required OBSERVED evidence.
- [ ] Coder harness forbids guessing required missing runtime values.
- [ ] Coder/Internal Fix cannot write Tester-owned workspace.

### D3 — repair from Tester failure
- [ ] PRODUCT_FAILURE routes to Coder/Internal Fix.
- [ ] Repair passes lightweight self-check.
- [ ] Reviewer must PASS repaired source.
- [ ] Only then does the same checkpoint become RETEST-ready.

Gate:
- Tester FAIL never bypasses Reviewer.

## E. Phase 9 Verification Controller / Local CI impact

### E1 — narrow authority
- [ ] Keep deterministic build/test/lint/typecheck discovery.
- [ ] Keep actual command evidence and false-PASS prevention.
- [ ] Remove/avoid authority to decide where Tester appears.
- [ ] Treat Verification Controller as a capability/evidence primitive usable by Coder self-check and declared Tester checkpoints.

### E2 — reusable verification adapters
- [ ] Expose safe fixed-argv verification commands for Tester checkpoints when relevant.
- [ ] Keep TEST_NOT_APPLICABLE and TEST_BLOCKED distinct from PASS.
- [ ] Never execute model-authored arbitrary shell as trusted verification.

Gate:
- checkpoint placement comes only from ExecutionGraph.

## F. Phase 10 Tester checkpoint subsystem

### F1 — workspace
- [ ] Create `.gsa/tester/<graph>/<checkpoint>/<attempt>/`.
- [ ] Tester may write plan/tests/fixtures/artifacts/report only there.
- [ ] Product root remains read-only.
- [ ] Coder/Internal Fix cannot mutate Tester workspace.
- [ ] Protect traversal/symlink/hardlink boundaries.

### F2 — Tester lifecycle
- [ ] GROUND.
- [ ] PLAN.
- [ ] AUTHOR.
- [ ] EXECUTE.
- [ ] ANALYZE.
- [ ] ADAPT.
- [ ] REPORT.
- [ ] RETEST.

### F3 — VERIFY
- [ ] PASS / FAIL / BLOCKED / NEEDS_HUMAN.
- [ ] UNVERIFIED != PASS.
- [ ] PRODUCT_FAILURE / TEST_FAILURE / ENVIRONMENT_FAILURE / NOT_READY / SPEC_GAP classification.

### F4 — MEASURE / PROBE
- [ ] COMPLETE / BLOCKED / NEEDS_HUMAN unless an approved threshold makes the result gating.
- [ ] Record declared variables/dimensions.
- [ ] Record sample index and boundary/change event.
- [ ] Record actual observed values + units.
- [ ] Record target revision/change_set.
- [ ] Record observable runtime/tool/capability identity.
- [ ] Record limitations and evidence refs.
- [ ] Do not fabricate thresholds or product verdicts.

### F5 — Tester sandbox and replay safety
- [ ] cwd = exact attempt workspace.
- [ ] writable roots = attempt workspace + runtime temp only.
- [ ] product root = readable, not writable.
- [ ] fixed/approved argv adapters only.
- [ ] bounded timeout/output.
- [ ] network policy explicit.
- [ ] Every executable step/adapter declares OBSERVE_ONLY / IDEMPOTENT / NON_IDEMPOTENT or equivalent.
- [ ] Persist execution fence/idempotency metadata when the adapter supports it.
- [ ] Uncertain NON_IDEMPOTENT work after crash must never auto-replay; route BLOCKED/NEEDS_HUMAN or explicit recovery.
- [ ] sandbox unavailable => BLOCKED, never unsandboxed fallback.

### F6 — evidence outputs
- [ ] OBSERVED.
- [ ] IMPLICATION.
- [ ] UNRESOLVED.
- [ ] Add EvidenceApplicability for each reusable output.
- [ ] Declare dependent product/config/runtime/environment dimensions.
- [ ] Declare invalidating changes and revalidation policy.
- [ ] Applicability uses a finite allowlisted matcher schema evaluated by runtime, not free-form model text.
- [ ] Only compatible OBSERVED evidence may satisfy a required measured input.
- [ ] Persist stable refs/hashes.

### F7 — crash-safe checkpoint resume
- [ ] Bind DUE/RUNNING/SATISFIED/BLOCKED/NEEDS_HUMAN to latest_checkpoint transactionally.
- [ ] Persist exact checkpoint + attempt identity.
- [ ] Resume a crashed RUNNING attempt by explicit recovery policy.
- [ ] Execution lease must prevent duplicate attempts.

### F8 — retest
- [ ] New reviewed target gets new attempt identity.
- [ ] Rerun prior failing tests.
- [ ] Rerun relevant regression.
- [ ] Old PASS/evidence cannot satisfy new target automatically.

## F9 — SPEC_GAP / replanning
- [ ] Tester may emit SPEC_GAP/PLAN_GAP evidence but cannot modify checkpoint topology.
- [ ] Material gap pauses current graph.
- [ ] Route to Planner/Human for a new PlanArtifact revision.
- [ ] New topology requires Reviewer + Local CR again.
- [ ] Job Builder registers a superseding graph only after approval.
- [ ] Non-material in-scope test adaptation does not force replanning.

## G. Phase 11 impact — Local CR / Job Pack completion

Do not implement yet.

Required future updates:
- [ ] CR packet includes checkpoint evidence relevant to Job Pack acceptance.
- [ ] Job Pack completion rejects unsatisfied required checkpoints.
- [ ] Job Pack completion rejects unresolved required empirical evidence.
- [ ] CR PASS alone still cannot mark DONE.

## H. Phase 12 impact — Milestone verification / resume

Do not implement yet.

Required future updates:
- [ ] milestone-level Test Checkpoints.
- [ ] resume DUE/RUNNING/BLOCKED/NEEDS_HUMAN checkpoint state.
- [ ] Milestone COMPLETE requires all required milestone checkpoints satisfied.
- [ ] downstream milestone activation resolves evidence dependencies first.

## I. Phase 13 impact — hardening

Add negative coverage:
- [ ] undeclared Tester invocation;
- [ ] checkpoint bypass;
- [ ] stale checkpoint evidence reuse;
- [ ] fake OBSERVED evidence;
- [ ] IMPLICATION used as measured input;
- [ ] guessed missing variable;
- [ ] invented MEASURE/PROBE threshold;
- [ ] Tester product-source write;
- [ ] Coder Tester-workspace write;
- [ ] Tester workspace traversal/symlink/hardlink escape;
- [ ] Tester process write-back to product root;
- [ ] unsandboxed fallback;
- [ ] misleading measurement without experiment variables;
- [ ] retest using old target evidence;
- [ ] restart duplicating checkpoint attempt.

## J. Cross-cutting Resume / Recovery hardening

Lessons source: WORKFLOW_RESUME_LESSONS.md.

### J1 — shared ResumeDecision contract
- [ ] Add a runtime-owned ResumeDecision / RecoveryClassification model.
- [ ] Resolve durable Project/Job/Task-or-JobPack/Run/workflow-stage state before choosing an agent.
- [ ] Resolve live source ref/revision and compare it with the last durable source target.
- [ ] Return exactly one next action: RESUME_CODER / RESUME_REVIEWER / RESUME_INTERNAL_FIX / RESUME_TESTER_ATTEMPT / RUN_REQUIRED_VERIFICATION / COMPLETE_RUN / WAIT_EXPLICIT_NEXT_TASK_START / BLOCKED_NEEDS_HUMAN.
- [ ] Conversation history must not be required to locate the next execution step.

### J2 — source versus durable-state reconciliation
- [ ] Classify source-ahead work as DURABLE_KNOWN / ADOPTABLE_SAME_SCOPE / SUPERSEDED_BY_NEW_PLAN / OUT_OF_SCOPE / UNKNOWN_REQUIRES_HUMAN.
- [ ] Persist an adoption/reconciliation record before recovered source is treated as current work.
- [ ] Never adopt a newer commit only because it is newer.
- [ ] Preserve the Human distinction between obsolete interrupted work and legitimate same-scope interrupted work.

### J3 — immutable Run identity and split target binding
- [ ] Run id/handle/path/parent lineage are runtime-owned and immutable.
- [ ] Separate immutable input_target from observed result_target.
- [ ] Reviewer/Goal Recheck bind to result_target.
- [ ] Remove any workflow need to manually rewrite target_revision/target_ref after a source mutation.

### J4 — canonical gate evidence
- [ ] Maintain canonical latest Goal Recheck / Review / Test / required CI pointers per active Run or workflow unit.
- [ ] New exact-target evidence supersedes the canonical pointer transactionally while append-only history remains preserved.
- [ ] Completion gates consume canonical semantic state rather than model-maintained ref arrays.
- [ ] Required UNVERIFIED/BLOCKED verification prevents terminal PASS.

### J5 — Local workflow entry resolvers
- [ ] Add resolve_planning_entry(); begin_plan_workflow may initialize only absent/new planning state.
- [ ] Add resolve_code_entry(); begin_code_workflow may initialize only absent/new code state.
- [ ] Resume REVIEWER/INTERNAL_FIX/PAUSED/REVIEW_PASS from durable state without resetting attempts/checklists/TODO/change-set identity.
- [ ] Reconstruct the exact next agent packet from Registry after process restart.
- [ ] Persist any minimal mutation/evidence digest required to reconstruct safe continuation without storing chat transcript/hidden reasoning.

### J6 — Online interrupted-Run recovery
- [ ] Add an open-Run/resume resolver distinct from Project lifecycle resume.
- [ ] Resolve RESUME_EXISTING / START_RECOVERY_CHILD / START_NEW_RUN / BLOCKED_RECONCILIATION_REQUIRED / ALREADY_TERMINAL.
- [ ] Detect orphan/incomplete Run, Verification and Handoff records after interrupted writes.
- [ ] Reject path/content Run identity mismatch before persistence.

### J7 — Handoff is not activation
- [ ] Add explicit activation_policy = EXPLICIT_START / AUTO_CONTINUE.
- [ ] Phase/task boundaries default to EXPLICIT_START unless the approved plan explicitly permits auto-continue.
- [ ] Handoff availability must never by itself activate the next Task/phase.

### J8 — structured evidence lifecycle
- [ ] Add CURRENT / SUPERSEDED / INVALIDATED evidence state with revision applicability.
- [ ] Preserve historical evidence while excluding stale evidence from current plan/job/resume packets.
- [ ] Human decisions that invalidate old assumptions must produce explicit evidence invalidation instead of prose cleanup only.

### J9 — restart test matrix
- [ ] Restart during every Planner/Reviewer/CR/Internal-Fix state.
- [ ] Restart during Coder before mutation, after mutation, after checkpoint and before Reviewer.
- [ ] Restart after Reviewer PASS but before required deterministic verification.
- [ ] Restart at Tester DUE/RUNNING/PREPARED/report/BLOCKED/SATISFIED states.
- [ ] Restart with source ahead of durable state.
- [ ] Restart with stale/unknown Run target.
- [ ] Restart with Handoff available while next Task requires explicit start.
- [ ] Assert no reset, duplicate non-idempotent action, skipped gate, stale evidence reuse or wrong next role.

Gate:
- a fresh process can recover the exact next safe action from durable state + live source alone;
- no manual JSON identity/target/ref repair is required;
- resume never silently advances into the next Human-bounded Task/phase.

## Execution order before coding resumes

1. Reconcile architecture docs and superseded partial Phase 10 code assumptions.
2. Add PlanArtifact EvidenceNeed + submit_plan contract.
3. Update ExecutionGraph + Job Builder checkpoint/evidence topology, including multi-JobPack/milestone prerequisites.
4. Update Registry checkpoint/evidence/applicability state.
5. Update Controller checkpoint/evidence dependency resolution.
6. Update harness contracts.
7. Build Tester workspace + evidence model.
8. Build Tester sandbox/execution loop.
9. Add checkpoint orchestration + transactional resume.
10. Add retest + downstream evidence injection.
11. Add SPEC_GAP → new plan revision → Reviewer/CR → superseding graph path.
12. Run regression/CI.
13. Reviewer.
14. Stop. Do not enter Phase 11 automatically.
