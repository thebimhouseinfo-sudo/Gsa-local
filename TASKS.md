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
- [ ] New topology requires Reviewer PASS followed by automatic Local CR on the exact plan revision/hash before Job Builder may register the superseding graph.
- [ ] Job Builder registers a superseding graph only after approval.
- [ ] Non-material in-scope test adaptation does not force replanning.

## G. Phase 11 impact — Task/Checkpoint completion + automatic Local CR

Do not implement yet.

Required future updates:
- [ ] Local CR runs automatically at Task terminal boundaries and at declared integration/checkpoint boundaries, after required Reviewer/verification/Tester evidence for that boundary is available.
- [ ] CR receives a fresh/stateless packet bound to the exact Task/Job Pack/revision/change_set plus required checkpoint evidence.
- [ ] CR does not run inside every Coder edit/self-check iteration; the normal repair loop remains Coder -> self-check -> Reviewer/Internal Fix until the boundary is mature.
- [ ] CR finding routes back through owner repair -> self-check -> Reviewer -> affected verification/Tester checkpoint -> CR; no Fix -> CR shortcut.
- [ ] Job Pack completion rejects unsatisfied required checkpoints, unresolved required empirical evidence, or missing required CR PASS for its terminal boundary.
- [ ] CR evidence alone cannot mark DONE; Orchestrator/Registry terminalizes only after all declared gates are compatible.

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

Lessons source: WORKFLOW_LESSONS_LEARNED.md.

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

### J6 — Online comparison only (non-executable in GSA Local)
- Online GSA also needs an interrupted-Run resolver, but implementation belongs to GPT-supper-agent, not this repository.
- Local may reuse the semantic lessons only: authoritative work cursor, crash-consistent finalization, exact target binding and explicit recovery classification.
- Do not create Online control-plane code/tasks from the GSA Local plan.

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

## Execution boundary before any coding resumes

Current Human-directed architecture change is itself a rebaseline event.

Current safe boundary:
- T-EXECUTION has passed review + executable CI evidence.
- T-ORCHESTRATION has not started.
- J-177F durable planning_revision 2 remains IN_PROGRESS and contains stale pre-migration governance semantics.

Therefore:
1. FREEZE J-177F before T-ORCHESTRATION. Do not execute additional J-177F tasks under the old topology.
2. Capture the live SourceTargetSet and exact evidence refs for work completed through T-EXECUTION.
3. Classify completed work as ADOPT_AS_IS / ADOPT_REVIEW_REQUIRED / ADAPT / SUPERSEDED; preserve source by default.
4. Revalidate T-EXECUTION and prerequisite evidence only on dimensions materially affected by the new architecture; do not discard valid CI/review evidence wholesale.
5. Create a replacement remaining-work Job/graph for T-ORCHESTRATION onward under the migrated governance and recovery contracts.
6. Map adopted prior tasks/runs/change sets into that replacement Job explicitly.
7. Only after the replacement plan passes Reviewer may coding resume.
8. Local CR remains automatic at mature Task/checkpoint/plan-finalization boundaries, never inside each edit loop; CR must use fresh/stateless exact-target evidence.

Operational note:
- current control plane does not yet support SUSPENDED_BY_REBASELINE; do not manually edit J-177F JSON to fake that state;
- the Human stop + this reviewed plan act as the operational freeze until Phase 14/15 runtime support exists.

This bridge applies the Rebaseline/Salvage contract now while Phase 14/15 later automate it.

## J2. Local CR boundary contract

Local CR is an independent model/contract, not Coder self-review and not the normal Reviewer repair loop.

- [ ] Planning: Planner <-> Reviewer until PASS, then automatic Local CR on exact revision/hash before PLAN_APPROVED.
- [ ] Code Task: Coder -> lightweight self-check -> Reviewer/Internal Fix loop; after Task is mature and required verification/Tester checkpoint evidence is ready, automatic Local CR runs before Task terminalization.
- [ ] Declared integration/milestone checkpoints may also require automatic Local CR after their required evidence gates.
- [ ] Coalesce overlapping boundaries: when a Task terminal and declared checkpoint refer to the same exact target/change_set, dispatch one CR packet with the union of required evidence, not duplicate CR runs.
- [ ] Persist a CRBoundaryKey (boundary identity + exact target/revision/change_set + plan/graph identity) so retry/resume cannot dispatch the same CR boundary twice.
- [ ] CR PASS is exact-target/revision/change_set bound and stale CR evidence cannot satisfy a newer target.
- [ ] CR finding never routes directly to CR retry: repair -> self-check -> Reviewer -> affected verification/Tester -> CR.
- [ ] CR runs fresh/stateless from durable source/evidence and does not inherit Coder/Reviewer hidden reasoning.
- [ ] Human remains authority for material product/architecture decisions, but Human invocation is not required to dispatch Local CR.

## K. Workflow Recovery / Rebaseline v2 migration

Sources (read-only):
- WORKFLOW_LESSONS_LEARNED.md
- GSA_LOCAL_UI_TESTER_TARGET_ARCHITECTURE.md

These tasks are future runtime architecture work. The immediate operational rebaseline above must happen before any more coding; K tasks later automate that behavior.

### K0 — project-first resolution
Dependencies: none.
- [ ] Resolve exact Project identity and registered PRIMARY/source repository before any Job/Run selection.
- [ ] Ambiguous Project resolution returns explicit candidates and blocks execution.
- [ ] Never infer Project from newest Job/Run or conversation recency.

No K-Q task may execute inside the frozen J-177F. Any compatibility prerequisite needed by the remaining Tester work must be planned explicitly in the replacement remaining-work Job/graph.

### K1 — authoritative WorkCursor / ResumeDescriptor
Dependencies: K0.

- [ ] Add one runtime-owned WorkCursor per active Job execution lineage.
- [ ] Bind Project, Job, plan revision/hash, execution graph version, Task/JobPack, active Run, workflow stage, pending gate, continuation policy and SourceTargetSet.
- [ ] Resume reads WorkCursor first; it must not infer the active Run by newest timestamp/commit.
- [ ] Reconcile multiple legacy IN_PROGRESS Runs before assigning the cursor.

### K2 — start vs resume vs recover
Dependencies: K1.

- [ ] Separate START / RESUME / RECOVER operations.
- [ ] Planning and Coding entry resolvers must inspect durable state before any begin_* initializer.
- [ ] begin_* may initialize only genuinely absent/new state.
- [ ] Attempt/review/checklist/TODO/change-set counters survive restart.

### K3 — crash-consistent durable transitions
Dependencies: K1, K2.

- [ ] Introduce transition_id/idempotency_key for Run/Verification/Handoff/finalization writes.
- [ ] Make interrupted partial persistence recoverable without duplicate durable records.
- [ ] Add durable ExternalOperation identity for CI, browser and external UI-provider operations.
- [ ] Repeated finalization converges to one terminal state.

### K3a — parent/child Run reconciliation
Dependencies: K1, K3.
- [ ] Model CONTINUATION / CORRECTION / REVIEW / RETEST / RECOVERY / SUPERSEDING_RUN lineage.
- [ ] Child terminalization reconciles the parent into COMPLETED / SUPERSEDED_BY_CHILD / FAILED / explicit-active-next-stage.
- [ ] No zombie IN_PROGRESS parent remains after ownership of continuation moves to a child.

### K3b — machine-readable gate diagnostics
Dependencies: K1, K3.
- [ ] Every blocked completion/resume gate returns GateDiagnostic with expected producer/run/target/result/lifecycle and observed refs/state.
- [ ] Agent never needs to guess whether mismatch is run id, target, producer, verification, lifecycle, path or freshness.
- [ ] GateDiagnostic is durable enough to reconstruct the next repair/recovery action.

### K3c — workflow replay policy + verification readiness
Dependencies: K1, K3.
- [ ] Classify workflow operations: IDEMPOTENT / IDEMPOTENT_BY_KEY / CHANGESET_BOUND / CAS_SERIALIZED / NON_IDEMPOTENT.
- [ ] Run creation, Verification/Handoff creation, source mutation, Job transitions, CI/deploy/browser/external-UI-provider operations each declare replay policy.
- [ ] Discover deterministic verification readiness early: command, execution surface, provider, OS/network constraints and AVAILABLE / NOT_CONFIGURED / ENVIRONMENT_BLOCKED / NOT_APPLICABLE.
- [ ] Verifier is read-only to product source by default; verification failure routes to Coder rather than auto-format/commit/push.
### K4 — source mutation write-ahead
Dependencies: K1, K3c.

- [ ] Add MutationIntent PREPARED/APPLIED/CHECKPOINTED/ABORTED.
- [ ] Bind expected-before and intended-after hashes.
- [ ] Use atomic replace where applicable.
- [ ] Recover source-ahead mutations without forensic guesswork.

### K5 — lease fencing
Dependencies: K1, K2.

- [ ] Replace PID-only lease identity with owner nonce + process identity + monotonic fencing token/epoch.
- [ ] Every mutating Registry transition validates the current fencing token.
- [ ] Stale owners cannot mutate after takeover.

### K6 — schema evolution
Dependencies: K1.

- [ ] Add durable schema/version compatibility contract.
- [ ] Migrations are ordered, idempotent and transactionally recorded.
- [ ] Compatible migrations preserve active workflow identity/budgets/evidence lineage.
- [ ] Semantic incompatibility routes to explicit stale/rebaseline state rather than silent reset.

### K7 — Human architecture rebaseline + salvage
Dependencies: K1, K2, K3a, K6, K9.

- [ ] Add HUMAN_REBASELINE_REQUIRED / REBASELINING workflow boundary.
- [ ] FREEZE affected work and capture source/Run/evidence snapshot.
- [ ] Classify prior work: ADOPT_AS_IS / ADOPT_REVIEW_REQUIRED / ADAPT_TO_NEW_CONTRACT / PARTIAL_SALVAGE / SUPERSEDED / REVERT_REQUIRED / FOREIGN_TASK / USER_OWNED / UNKNOWN.
- [ ] Propagate dependency impact from changed upstream contracts.
- [ ] New plan starts from frozen live SourceTargetSet + accepted salvage decisions.
- [ ] Old Runs receive explicit rebaseline/supersession semantics; no blanket revert/reset.

### K8 — bounded integration checkpoints
Dependencies: K1, K7, K9.

- [ ] Add AcceptedIntegrationBaseline at coherent reviewed/verified boundaries.
- [ ] Distinguish READY_FOR_INTEGRATION from AUTHORIZED_TO_MERGE.
- [ ] Direct-main repositories still record stable accepted revisions.
- [ ] Multi-repo integration may be partial but must be explicit.
- [ ] Architecture rebaseline uses the latest accepted baseline as a stable anchor.

### K9 — multi-repo SourceTargetSet
Dependencies: K1.

- [ ] Replace singular source-target assumptions with SourceTargetSet.
- [ ] Bind review/evidence to exact repo/ref/revision combinations.
- [ ] Bind WorkCursor to plan_revision + plan_hash + graph_version.
- [ ] Detect PARTIAL_SOURCE_ADVANCE / STALE_GRAPH_BINDING / SOURCE_SET_CONFLICT.

### K10 — recovery regression matrix
Dependencies: K0, K1, K2, K3, K3a, K3b, K3c, K4, K5, K6, K7, K8, K9.

- [ ] Restart before/after mutation, checkpoint, Reviewer, Internal Fix, Verification and Handoff.
- [ ] Recover partial durable writes idempotently.
- [ ] Recover source-ahead known-lineage work.
- [ ] Block unknown-lineage adoption.
- [ ] Preserve monotonic retry/attempt budgets across restart.
- [ ] Ensure Human stop/EXPLICIT_START remains durable.
- [ ] Project-first resolution blocks ambiguous/wrong Project continuation.
- [ ] Parent/child Run recovery leaves no zombie IN_PROGRESS parent.
- [ ] GateDiagnostic identifies exact mismatch fields needed for repair/recovery.
- [ ] Workflow replay/readiness tests cover idempotent-by-key, CAS/change-set-bound and uncertain NON_IDEMPOTENT operations.

## K11. Workflow-lessons migration coverage

| Lessons | Owner | Required executable contract |
|---|---|---|
| 1-6, 9, 12, 16-18, 20-25, 30-32, 37, 40 | K0-K2, K9 | WorkCursor/ResumeDecision, exact target lineage, no chat-history dependency, SourceTargetSet |
| 7, 33 | K3 | transition identity + idempotent finalization |
| 8 | K3a | parent/child Run reconciliation, no zombie Run |
| 11, 28 | K3b + canonical evidence | machine-readable blocking diagnostics + runtime current evidence pointer |
| 14, 19, 27, 35-36 | K3c | workflow replay policy, verification readiness, ExternalOperation, monotonic budgets |
| 13, 34 | K5 | execution identity + fencing token |
| 26 | K1/K2 | durable EXPLICIT_START / activation policy |
| 29 | K0 | Project resolution before Job resolution |
| 38 | K4 | mutation write-ahead |
| 39 | K6 | durable schema migration |
| 41 | immediate bridge + K7 | Human Rebaseline/Salvage |
| 42 | K8 | AcceptedIntegrationBaseline / bounded integration |

Gate: no lesson is considered migrated merely because it is mentioned in prose; each row must terminate in a runtime contract, test or explicit Human governance boundary.
## L. UI architecture rebaseline

Local UI execution uses runnable source + Vercel agent-browser as the primary implementation/observation loop. No external design-authoring application is required by the workflow.

### L1 — role model
Dependencies: K10.

- [ ] Keep Designer plus UX Coder/UI Coder specialization contracts.
- [ ] Planner owns product/UX intent and material workflow decisions.
- [ ] Designer owns unresolved visual intent/direction/specification when needed.
- [ ] UI Coder owns implementation HOW, browser-driven refinement and UI-specific source work.
- [ ] Tester independently verifies the runnable UI.
- [ ] Human retains material visual acceptance and authority to discard/rebuild an existing shell.

### L2 — preserve product workflow classification
Dependencies: L1.

- [ ] Keep UI_FIRST / UX_FIRST as the product workflow classification.
- [ ] Do not add a separate design-tool execution mode.
- [ ] UI_FIRST/UX_FIRST controls when UX/visual requirements stabilize; both ultimately execute UI through runnable source + agent-browser.
- [ ] UX_FIRST may use TEMPORARY_UI and durable UIRequirement collection before a larger UI Update Pack.

### L3 — visual authority contract
Dependencies: L2.

- [ ] Resolve visual_authority_source before UI implementation: EXISTING_APPROVED_SHELL / EXISTING_DESIGN_SYSTEM / DESIGNER_SPEC / HUMAN_DIRECTION.
- [ ] UI Coder must not invent unresolved material visual decisions.
- [ ] If current shell/design system/Human direction is sufficient, UI Coder may proceed directly.
- [ ] Otherwise Designer supplies material visual direction/specification before UI Coder mutates the affected source; only non-material refinement may be clarified during the UI Coder loop.
- [ ] Designer output is a design contract, not a dependency on any fixed design application.
- [ ] Human subjective acceptance remains separate from objective browser verification.

### L4 — existing shell vs shell rebuild
Dependencies: L3.

- [ ] Any existing product shell is preserved by default, even if aesthetically poor.
- [ ] UI Coder may substantially improve layout/responsive/header/cards/navigation/typography/states directly in source.
- [ ] A broken/non-runnable application is a repair/integration issue, not authority to discard the shell.
- [ ] Discarding/rebuilding/replacing an existing shell requires explicit Human approval.
- [ ] After Human approves a rebuild, freeze/rebaseline/salvage the affected existing shell/work first; only then may UI Coder replace it in runnable source with agent-browser.
- [ ] Greenfield also uses the same source + browser UI workflow; complexity only changes Designer/Human checkpoint depth, not the execution technology.

### L5 — asset and visual-resource contract
Dependencies: L4.

- [ ] Preserve reviewed asset ID/filename/prompt/usage/ratio contracts when assets are required.
- [ ] UI Coder may use source-native primitives/icons when they satisfy the visual contract.
- [ ] UI Coder must not silently replace explicit artwork/asset requirements with generic primitives.
- [ ] Asset generation/acquisition remains capability-gated and independently attributable.
- [ ] No specific icon/font/asset provider is mandatory in the core workflow.

## M. Vercel agent-browser for UI Coder and Tester

### M1 — shared browser executor
Dependencies: K10 and rebaselined Tester checkpoint subsystem.

- [ ] Add Vercel agent-browser MCP as the primary browser observation/execution layer for Local UI work.
- [ ] UI Coder uses it for development feedback/self-check.
- [ ] Tester uses it independently for checkpoint verification.
- [ ] Preserve separate sessions/operation identities and evidence authority for UI Coder vs Tester.
- [ ] Enforce open -> wait -> snapshot -> interact -> re-snapshot -> observe -> screenshot/report discipline.
- [ ] Treat stale element refs as TEST_FAILURE in Tester runs and invalid development refs in UI Coder loops.

### M2 — standalone Local agent-browser PROBE
Dependencies: M1.

- [ ] Prove launch, target open, load wait, snapshot, click/fill, re-snapshot, viewport handling, screenshot, diff where required, evidence path, console/page error collection and deterministic cleanup.
- [ ] Probe state/session save/load only where required.
- [ ] Prove separate UI Coder and Tester sessions can target the same runnable app without sharing verification authority.
- [ ] Missing required operation => INTEGRATION_NOT_READY; tool presence never implies FUNCTIONAL.

### M3 — Tester browser evidence workspace
Dependencies: M2.

- [ ] Store Tester browser sessions/snapshots/screenshots/diffs/console evidence in Tester-owned workspace.
- [ ] Product source remains read-only to Tester.
- [ ] Bind artifacts to checkpoint, exact runnable target, viewport and browser operation identity.
- [ ] Surface direct screenshot/diff paths to Human; no manual directory hunting.

### M4 — Tester UI checkpoint semantics
Dependencies: M2, M3.

- [ ] Tester independently plans the checkpoint.
- [ ] Verify the exact runnable source target with an independent agent-browser session.
- [ ] Verify objective screen/state/flow/responsive/clipping/asset/runtime criteria only through capabilities actually available for that target.
- [ ] UI Coder self-check artifacts may be context but cannot satisfy Tester verification.
- [ ] Subjective visual judgement routes to NEEDS_HUMAN.
- [ ] Tester evidence follows dimension-aware applicability.

## N. UI Coder browser-driven workflow

### N1 — UI Coder-owned state machine
Dependencies: L5, M2.

- [ ] UI Coder owns only UI_GROUND -> UI_IMPLEMENT -> UI_RENDER -> UI_INSPECT -> UI_REFINE loop -> UI_SELF_CHECK -> UI_HANDOFF_READY.
- [ ] UI_REFINE may loop without Reviewer while UI Coder is still inside the same approved visual/source scope.
- [ ] UI Coder cannot self-advance REVIEW, TEST, HUMAN_REVIEW, UI_ACCEPTED or SHELL_READY.
- [ ] Material UX/flow/state/architecture changes leave the refine loop and route back to Planner/Human.
- [ ] Human-approved shell rebuild remains explicit durable context throughout the Run.
- [ ] Durable UI cursor records stage_owner and producer/run identity for every transition.

### N2 — source + browser development loop
Dependencies: N1.

- [ ] UI Coder changes runnable source, starts/uses the real app, then inspects the rendered result with agent-browser.
- [ ] Use snapshot/screenshot/diff/interaction/viewport evidence as development feedback.
- [ ] Check layout, responsive behavior, clipping/overflow, visible states, navigation and interaction before UI_SELF_CHECK.
- [ ] Compile/test success alone never proves UI completion.
- [ ] UI Coder browser evidence is development/self-check evidence only; it cannot satisfy Tester PASS.

### N2a — post-self-check orchestration gates
Dependencies: N2, M4.

- [ ] Orchestrator, not UI Coder, transitions UI_HANDOFF_READY -> REVIEW_PENDING.
- [ ] Reviewer owns REVIEWING/result production; CHANGES_REQUIRED routes a new UI Coder correction/refine Run, PASS returns control to Orchestrator.
- [ ] When a Tester checkpoint is declared, Orchestrator transitions REVIEW_PASS -> TEST_PENDING; Tester owns TESTING/result production.
- [ ] PRODUCT_FAILURE routes through UI Coder correction -> UI_SELF_CHECK -> Reviewer before retest.
- [ ] HUMAN_REVIEW_PENDING is created only when the plan declares subjective/material Human acceptance.
- [ ] UI_ACCEPTED and SHELL_READY are Orchestrator terminalization decisions from compatible Reviewer/Tester/Human evidence; no child role self-declares them.
- [ ] Resume dispatches by durable stage_owner/producer rather than by last chat role.

### N3 — UI Coder workspace
Dependencies: N1.

- [ ] Give UI Coder a dedicated design/workspace area separate from production source ownership, e.g. .ui-design/ or equivalent runtime workspace.
- [ ] Workspace may contain briefs, references, screenshots, comparisons, asset manifests, prompts, tokens, notes and temporary prototypes.
- [ ] Workspace drafts/prototypes are non-runtime and must not be imported, bundled or served by the product directly.
- [ ] Any asset promoted from UI workspace into production moves through the canonical product asset path with provenance and required approval/evidence.
- [ ] Draft/prototype material is non-authoritative until implemented/reviewed in product source.
- [ ] Preserve direct links/paths from browser captures and visual references into the active UI Run.
- [ ] Resume reconstructs the UI Coder stage and relevant workspace refs without relying on chat history.

### N4 — shell-build/rebuild workflow
Dependencies: N2, L4, K7.

- [ ] For greenfield shell creation, use real source as the prototype surface.
- [ ] For an existing-shell rebuild, require Human approval plus completed Rebaseline/Salvage classification before destructive shell replacement begins.
- [ ] Designer/Human may define visual direction before implementation; UI Coder realizes it directly in source.
- [ ] UI Coder repeatedly render/inspect/refine with agent-browser until the shell reaches UI_SELF_CHECK.
- [ ] Human may review runnable screenshots/live result at meaningful shell checkpoints.
- [ ] Do not create a parallel mockup authority that can drift from production source.

## O. UI review / Human loop / SHELL_READY

### O1 — normal UI change flow
Dependencies: N2, N2a, M4.

- [ ] Planner/Designer visual authority -> UI Coder source/browser loop -> UI_SELF_CHECK -> Reviewer -> Tester independent agent-browser.
- [ ] Human evidence review is added only where product plan or subjective visual acceptance requires it.
- [ ] Routine visual refinements within approved authority return to UI Coder without unnecessary architecture replanning.
- [ ] Material UX/flow/state/shell-architecture changes route to Planner/Human.

### O2 — shell creation/rebuild flow
Dependencies: N4, N2a, M4.

- [ ] Greenfield shell creation or Human-approved shell rebuild uses the same source/browser loop.
- [ ] Use representative content and responsive states while the shell is still cheap to change.
- [ ] Human reviews meaningful runnable shell checkpoints rather than a separate design-tool artifact.
- [ ] Reviewer and Tester verify the implemented source before shell acceptance.

### O3 — representative shell pilot
Dependencies: O1 or O2.

- [ ] Use representative real content before broad scale-out when shell risk justifies a pilot.
- [ ] Include long/short/media/error/loading/responsive stress cases as applicable.
- [ ] Tester verifies runnable shell with independent agent-browser.

### O4 — UI_ACCEPTED / SHELL_READY
Dependencies: N2a plus O3 when shell-level readiness is required; otherwise N2a + O1.

- [ ] Ordinary bounded UI changes may terminate at UI_ACCEPTED after declared review/test/Human gates.
- [ ] SHELL_READY is reserved for greenfield/rebuilt/material shell baselines where broad scale-out depends on shell stability.
- [ ] SHELL_READY requires runtime evidence.
- [ ] SHELL_READY becomes an AcceptedIntegrationBaseline candidate.
- [ ] No separate design-tool approval can substitute for runnable-source evidence.

## P. UX_FIRST UI Update Packs

### P1 — durable UI requirements
Dependencies: L2.

- [ ] Record UIRequirement with source Job/Task, affected surface, reason, evidence and provisional UI ref.
- [ ] Temporary UI is explicitly provisional.
- [ ] Existing shell remains the baseline unless Human explicitly approves a shell rebuild.

### P2 — UI Update Pack execution
Dependencies: P1, L3, N2, N2a, M4.

- [ ] Planner groups coherent requirements by affected shell/surface.
- [ ] Designer resolves material visual direction when current authority is insufficient.
- [ ] UI Coder implements the pack directly in source using the browser-driven loop.
- [ ] Reviewer + Tester verify the implemented target.
- [ ] Avoid redesign after every UX change and avoid permanent temporary UI.

## Q. UI capability-provider expansion

Core UI workflow must remain functional with source + agent-browser alone. Additional providers/skills are optional extensions.

### Q1 — provider abstraction
Dependencies: N2.

- [ ] Add capability slots/selection rules rather than hard-coding one UI resource provider.
- [ ] Candidate future capabilities may include icon libraries, font libraries, component libraries, open/stock asset sources, image generation, SVG/vector tooling and asset optimization.
- [ ] Provider absence must not block core UI work unless the approved UI contract explicitly requires that resource type.
- [ ] Record provider/version/source identity for generated or imported durable assets when relevant.

### Q2 — UI skills
Dependencies: N2.

- [ ] Add specialized skills progressively: responsive/layout, accessibility, design-token handling, framework-specific UI, visual-regression interpretation and asset integration.
- [ ] Skills extend UI Coder reasoning/implementation but do not change Human/Reviewer/Tester authority.
- [ ] Capability/skill availability is discovered, not assumed.

## R. UI architecture pilot
Dependencies: K10, L5, N4, M4, O4, P2.

- [ ] Run one brownfield UI Coder browser-loop pilot against an existing shell.
- [ ] Run one simple greenfield shell pilot entirely in runnable source.
- [ ] Exercise a shell rebuild transition only on a genuine Human-approved rebuild case or controlled fixture/sandbox; never manufacture a product rebuild solely for workflow coverage.
- [ ] Verify UI Coder development session and Tester independent session remain separate.
- [ ] Verify Designer can supply visual direction without any fixed design application.
- [ ] Verify resume across UI Coder-owned UI_GROUND / UI_IMPLEMENT / UI_RENDER / UI_INSPECT / UI_REFINE / UI_SELF_CHECK / UI_HANDOFF_READY and orchestration-owned REVIEW_PENDING / TEST_PENDING / HUMAN_REVIEW_PENDING / terminalization stages.
- [ ] Verify Rebaseline/Salvage on a genuine or controlled shell-rebuild transition.
- [ ] Verify the core source + browser UI workflow works with no optional provider configured.
- [ ] After Q1 exists, verify provider-selection fallback/absence behavior separately without making Q1 a prerequisite for the core pilot.
- [ ] Verify bounded integration baselines prevent unbounded commit accumulation.
- [ ] Calibrate before making the workflow mandatory across all Projects.
