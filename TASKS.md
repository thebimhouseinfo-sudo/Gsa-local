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
- [ ] New topology requires Reviewer again; CR remains Human-invoked only and is never auto-dispatched.
- [ ] Job Builder registers a superseding graph only after approval.
- [ ] Non-material in-scope test adaptation does not force replanning.

## G. Phase 11 impact — Job Pack completion / Human-invoked CR backstop

Do not implement yet.

Required future updates:
- [ ] If Human invokes CR, its packet includes checkpoint evidence relevant to the exact Job Pack/revision.
- [ ] Job Pack completion rejects unsatisfied required checkpoints.
- [ ] Job Pack completion rejects unresolved required empirical evidence.
- [ ] CR is never auto-dispatched; CR evidence alone cannot mark DONE.

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
8. CR remains Human-invoked only.

Operational note:
- current control plane does not yet support SUSPENDED_BY_REBASELINE; do not manually edit J-177F JSON to fake that state;
- the Human stop + this reviewed plan act as the operational freeze until Phase 14/15 runtime support exists.

This bridge applies the Rebaseline/Salvage contract now while Phase 14/15 later automate it.

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
- [ ] Add durable ExternalOperation identity for CI, Penpot and browser operations.
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
- [ ] Run creation, Verification/Handoff creation, source mutation, Job transitions, CI/deploy/browser/Penpot operations each declare replay policy.
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
## L. UI / Design architecture rebaseline

Target source: GSA_LOCAL_UI_TESTER_TARGET_ARCHITECTURE.md.

### L1 — role/workflow model
Dependencies: K10.

- [ ] Add Designer plus UX Coder/UI Coder specialization contracts.
- [ ] Preserve Planner functional intent, Designer visual intent, Coder source HOW.
- [ ] Preserve Tester independence and Human subjective acceptance authority.

### L2 — UI_FIRST / UX_FIRST planning contract
Dependencies: L1.

- [ ] Planner classifies every UI-bearing Project/Job as UI_FIRST or UX_FIRST.
- [ ] Define UX execution contract before visual architecture.
- [ ] UX_FIRST supports approved TEMPORARY_UI + durable UI requirement collection.
- [ ] Changing classification is a material Human decision.

### L3 — design readiness model
Dependencies: L2.

- [ ] Add Design Coverage, DESIGN_READY, UI DESIGN APPROVED and SHELL_READY semantics.
- [ ] Bind design baselines to exact design/source/evidence refs.
- [ ] Material design changes supersede prior baseline explicitly.

### L4 — asset contract
Dependencies: L3.

- [ ] Distinguish Penpot-native shell assets, reviewed generated-asset contracts and deep-content assets.
- [ ] Preserve reviewed asset ID/filename/prompt/usage/ratio contracts.
- [ ] Do not let UI Coder replace explicit generated artwork contracts with generic primitives without approval.

## M. UI Coder + Penpot Local integration

Generic UI Coder + Penpot capability has already PASSed in prior GSA capability testing. Do NOT repeat the broad capability experiment by default.

Before Local relies on that PASS:
- [ ] If durable upstream evidence exists, store it as UPSTREAM_OBSERVED_REF with explicit applicability.
- [ ] If only the Human-provided accepted fact is available, store HUMAN_ACCEPTED_EXTERNAL as a planning premise, not as OBSERVED runtime verification.
- [ ] Either form may skip the broad generic re-probe by Human decision, but neither satisfies Local reachability/readback/resume integration evidence.
- [ ] Revalidate only if evidence applicability becomes stale or Local uses a materially different execution surface.

### M1 — Local Penpot binding
Dependencies: K10, L4.

- [ ] Integrate the already-capable UI Coder/Penpot surface into Local.
- [ ] Verify Local reachability/invocation only.
- [ ] Persist Penpot project/file/object identity needed for resume.
- [ ] Verify readback/export/artifact paths across fresh Local process/session.
- [ ] Treat upstream Penpot evidence as reusable only while applicability remains valid.

### M2 — UI Design Phase
Dependencies: M1, L3.

- [ ] Insert UI DESIGN PHASE after DESIGN_READY/review and before source UI implementation when required.
- [ ] Require full screen coverage + representative content coverage.
- [ ] Allow Penpot components/tokens/responsive variants/assets/design-to-code as implementation aids.
- [ ] Do not narrow existing UI Coder source responsibilities.

### M3 — design baseline persistence
Dependencies: M2.

- [ ] Persist ui_design_baseline_id + Penpot identity/version + coverage + asset refs + Tester refs + Human approval.
- [ ] Resume exact UI_DESIGN / WAITING_HUMAN_DESIGN / UI_DESIGN_APPROVED stage.
- [ ] Human Rebaseline invalidates obsolete design authority without deleting reusable artifacts.

## N. Tester + Vercel agent-browser MCP

### N1 — executor integration
Dependencies: K10 and rebaselined Tester checkpoint subsystem.

- [ ] Add Vercel agent-browser MCP as primary browser executor for Local Tester.
- [ ] Keep Tester as the reasoning agent; browser MCP is execution only.
- [ ] Enforce open -> snapshot -> interact -> re-snapshot -> observe -> screenshot/report discipline.
- [ ] Treat stale element refs as TEST_FAILURE.

### N2 — standalone agent-browser PROBE
Dependencies: N1.

- [ ] Prove launch, target open, load wait, snapshot, click/fill, re-snapshot, viewport handling, screenshot, evidence path, console/page error collection and deterministic cleanup.
- [ ] Probe state/session save/load only where required.
- [ ] Missing operation => INTEGRATION_NOT_READY; tool presence never implies FUNCTIONAL.

### N3 — Tester evidence workspace
Dependencies: N2.

- [ ] Store browser sessions/snapshots/screenshots/diffs/console evidence in Tester-owned workspace.
- [ ] Product source remains read-only.
- [ ] Define immutable PenpotEvidenceBundle for non-browser design checkpoints: Penpot project/file/version identity, frame/screen inventory, exported artifact refs + hashes, responsive-frame metadata, provider operation id, captured_at and applicability context.
- [ ] PenpotEvidenceBundle is produced by runtime/connector readback/export or capability-gated Tester read-only Penpot access; UI Coder prose/self-report alone is not verification evidence.
- [ ] Bind all artifacts to checkpoint, exact target/design baseline, viewport where applicable and browser/Penpot operation identity.
- [ ] Surface direct screenshot/export paths to Human; no manual directory hunting.

### N4 — UI checkpoint semantics
Dependencies: N2, N3, M2.

- [ ] Tester independently plans the UI checkpoint.
- [ ] Use agent-browser only when the checkpoint has a browser-inspectable prototype or runnable target.
- [ ] If the Penpot artifact is not browser-inspectable, Tester consumes PenpotEvidenceBundle/read-only Penpot evidence plus Human review; do not fabricate browser verification.
- [ ] Verify objective screen/state/flow/responsive/clipping/asset/runtime criteria only through capabilities actually available for that target.
- [ ] Subjective visual judgement routes to NEEDS_HUMAN.
- [ ] Tester evidence follows dimension-aware applicability.

## O. Human design loop / SHELL_READY

### O1 — Human design loop
Dependencies: M3, N4.

- [ ] UI Coder + Penpot -> Tester evidence -> Human -> UI Coder revision -> Tester retest.
- [ ] Routine visual refinements stay inside the loop.
- [ ] Material flow/state/topology/asset/architecture changes trigger Planner/Rebaseline.

### O2 — source binding
Dependencies: O1, M3.

- [ ] UI source implementation must bind to UI DESIGN APPROVED baseline.
- [ ] Penpot design-to-code is an accelerator, not canonical source.
- [ ] Coder self-check + Reviewer remain required.

### O3 — representative shell pilot
Dependencies: O2, N4.

- [ ] Implement representative real-content pilot before scale-out.
- [ ] Include long/short/media/error/loading/responsive stress cases.
- [ ] Tester verifies runnable shell with agent-browser.

### O4 — SHELL_READY
Dependencies: O3.

- [ ] SHELL_READY requires runtime evidence, not design approval alone.
- [ ] SHELL_READY becomes an AcceptedIntegrationBaseline candidate.
- [ ] Only after SHELL_READY may broad screen/content scale-out proceed.

## P. UX_FIRST UI Update Packs

### P1 — durable UI requirements
Dependencies: O4, L2.

- [ ] Record UIRequirement with source Job/Task, affected surface, reason, evidence and provisional UI ref.
- [ ] Temporary UI is explicitly provisional.

### P2 — UI Update Pack
Dependencies: P1.

- [ ] Planner groups coherent requirements by affected shell/surface.
- [ ] Designer -> UI Coder/Penpot -> Tester -> Human -> source refinement.
- [ ] Avoid redesign after every UX change and avoid permanent temporary UI.

## Q. UI architecture pilot
Dependencies: K10, L4, M3, N4, O4, P2.


- [ ] Run one real UI_FIRST pilot.
- [ ] Run one UX_FIRST/update-pack pilot when appropriate.
- [ ] Verify resume at DESIGNER / UI_DESIGN / WAITING_HUMAN / UI_IMPLEMENTATION / SHELL_TEST stages.
- [ ] Verify Rebaseline/Salvage with an intentional Human architecture change.
- [ ] Verify bounded integration baselines prevent unbounded commit accumulation.
- [ ] Calibrate before making the workflow mandatory across all Projects.
