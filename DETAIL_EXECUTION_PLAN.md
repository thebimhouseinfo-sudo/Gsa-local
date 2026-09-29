# GSA Local — Detailed Execution Plan

This plan turns `IMPLEMENTATION_PLAN.md` into bounded implementation Jobs.

## Current architecture rebaseline — Tester checkpoint subsystem

The Tester redesign has cross-phase impact. Earlier phases are not rolled back wholesale; each keeps its proven core invariant and receives only the contract extensions required by the new Tester architecture.

### Impact map

| Phase | Impact | Required update |
|---|---|---|
| Phase 2 — Harness Engine | MEDIUM | Extend Planner, Job Builder, Tester and Coder role contracts for checkpoint planning and empirical-evidence provenance. |
| Phase 3 — Registry | MEDIUM | Extend schema for checkpoint definitions/state, Tester attempts, structured experiment samples, named evidence outputs and evidence requirements. Keep atomic transition/stale-write invariants. |
| Phase 4 — Checkpoint + Lease | HIGH | Tester DUE/RUNNING/BLOCKED/NEEDS_HUMAN/SATISFIED plus attempt identity must update latest_checkpoint transactionally and resume under the execution lease without duplicate attempts. |
| Phase 5 — Planner workflow | HIGH | Planner must persist first-class EvidenceNeed/UnknownRuntimeFact in PlanArtifact; evidence intent must survive plan hash/review/CR instead of living only in prose. |
| Phase 6 — Job Builder | HIGH | Job Builder must transform approved EvidenceNeed records into explicit TestCheckpointSpec + VERIFY/MEASURE/PROBE + named outputs + EvidenceRequirement edges, including multi-JobPack/milestone prerequisite topology. |
| Phase 7 — Milestone / Job Pack Controller | HIGH | Controller must recognize declared checkpoint boundaries and stop/resume at them without treating Reviewer PASS as an implicit Tester trigger. |
| Phase 8 — Coder ↔ Reviewer | MEDIUM | Core loop stays unchanged. Coder context gains required OBSERVED evidence; Coder/Internal Fix must be denied writes to Tester-owned workspace; repair after Tester FAIL still returns through Reviewer before retest. |
| Phase 9 — Verification Controller + Local CI | HIGH semantic change | Keep deterministic build/test capability discovery and Local CI evidence, but remove authority to decide where Tester appears. Phase 9 becomes a deterministic verification primitive/capability provider used by coding self-checks and declared checkpoints. |
| Phase 10 — Tester subsystem | MAJOR | New independent checkpoint subsystem with its own workspace, test planning, execution, VERIFY/MEASURE/PROBE, empirical evidence and retest. |
| Phase 11 — Code Local CR | MEDIUM | CR packet must consume checkpoint/evidence state relevant to final Job Pack acceptance, but CR remains after implementation/verification gates. |
| Phase 12 — Milestone Verification + Resume | HIGH | Milestone completion must require all declared milestone checkpoint requirements and must resume checkpoint state correctly after restart. |
| Phase 13 — Hardening | HIGH | Add negative tests for evidence spoofing/staleness, checkpoint bypass, Tester workspace escape, fake measurements, guessed required variables and unsafe Tester execution. |

### Important invariant changes

Old assumption:

```text
Reviewer PASS
 -> Verification Controller decides testability
 -> maybe Tester
```

New architecture:

```text
Approved Plan
 -> Job Builder declares checkpoint(s)
 -> Milestone/Job Pack execution reaches declared boundary
 -> Tester checkpoint runs
```

The normal coding loop remains:

```text
Coder
 -> lightweight self-check
 -> Reviewer
 -> Internal Fix when required
 -> Reviewer PASS
```

Tester is outside that loop.

### Evidence dependency rule

When later work depends on a runtime fact, Job Builder must encode a dependency on a named Tester evidence output.

```text
Checkpoint PROBE/MEASURE
 -> OBSERVED evidence
 -> EvidenceRequirement
 -> later Job Pack / Planner / Coder context
```

If required evidence is missing, stale or incompatible:

```text
BLOCKED / PLAN_GAP
```

Never invent a replacement value.

## Milestone A — Runtime foundation
Completed core:
- Rust crate, `gsa` binary, local CI.
- Config/model/Ollama foundation.
- Harness registry.
- SQLite registry/checkpoint/lease core.

Tester rebaseline impact:
- no foundation rollback;
- Registry and harness contracts receive additive Phase 10 schema/role extensions.

## Milestone B — Planning workflow and Job Builder
Completed core:
- revisioned Implementation Plan;
- Planner↔Reviewer;
- Local CR;
- PLAN_APPROVED binding;
- Job Builder execution-graph registration.

Tester rebaseline extensions:
- Planner persists unknown runtime facts/evidence needs as first-class PlanArtifact data;
- EvidenceNeed participates in plan revision/hash and Reviewer/CR approval;
- Job Builder transforms approved EvidenceNeed into Test Checkpoints/evidence dependencies rather than re-inferring intent from prose;
- Test Checkpoints may depend on one Job Pack, a Job Pack set, or a milestone integration boundary;
- `submit_execution_graph` schema supports checkpoint definitions and named evidence outputs;
- Job Builder returns PLAN_GAP when required checkpoint/evidence semantics cannot be derived safely.

## Milestone C — Milestone / Job Pack controller
Completed core:
- Job Pack states;
- dependency resolution;
- Milestone lock/unlock;
- checkpoint resume;
- active Job Pack context;
- no-jump enforcement.

Tester rebaseline extensions:
- resolve declared Test Checkpoint boundaries;
- persist checkpoint state separately from Job Pack DONE;
- block work past a checkpoint until required VERIFY/MEASURE/PROBE outputs are satisfied;
- inject required OBSERVED evidence into downstream ActiveWork.

## Milestone D — Coding / Verification / Tester
Completed core:
- Coder↔Reviewer/Internal Fix;
- deterministic Verification Controller;
- Local CI command discovery and evidence.

Rebased work:
1. keep Coder self-check + Reviewer loop independent;
2. narrow Verification Controller to deterministic verification/capability evidence;
3. implement Tester checkpoint subsystem;
4. implement Tester-owned workspace and sandbox;
5. implement exact-target empirical evidence;
6. implement evidence consumer contract;
7. implement repair → Reviewer PASS → RETEST;
8. integrate later with Local CR final gate only after Phase 10 is complete.

## Milestone E — Completion / Resume / Hardening
Future work must now include:
- checkpoint state crash/resume is already handled in Phase 10 through latest_checkpoint + lease;
- material Tester SPEC_GAP pauses the graph and requires new plan revision + Reviewer/CR + superseding graph;
- CR packet consumes required checkpoint evidence;
- Job Pack completion rejects unsatisfied required checkpoints;
- Milestone completion rejects unsatisfied milestone-level checkpoints;
- restart resumes DUE/RUNNING/BLOCKED/NEEDS_HUMAN Tester state;
- hardening covers evidence spoof/stale/context mismatch, checkpoint bypass, workspace escape and unsafe test execution.

Only the current approved Job is implemented at a time. Runtime state, not Markdown checkboxes, remains authority.
