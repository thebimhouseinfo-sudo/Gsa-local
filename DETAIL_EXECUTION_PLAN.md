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
| Phase 11 — Job Pack Completion / Human-invoked CR backstop | MEDIUM | If Human invokes CR, its packet consumes checkpoint/evidence state relevant to the exact Job Pack/revision; CR is not an automatic completion gate. |
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
- Human-invoked CR backstop when requested;
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
8. integrate later with Job Pack completion gates; CR remains Human-invoked only.

## Milestone E — Completion / Resume / Hardening
Future work must now include:
- checkpoint state crash/resume is already handled in Phase 10 through latest_checkpoint + lease;
- material Tester SPEC_GAP pauses the graph and requires new plan revision + Reviewer + any explicitly Human-invoked CR evidence + superseding graph;
- CR packet consumes required checkpoint evidence;
- Job Pack completion rejects unsatisfied required checkpoints;
- Milestone completion rejects unsatisfied milestone-level checkpoints;
- restart resumes DUE/RUNNING/BLOCKED/NEEDS_HUMAN Tester state;
- hardening covers evidence spoof/stale/context mismatch, checkpoint bypass, workspace escape and unsafe test execution.

Only the current approved Job is implemented at a time. Runtime state, not Markdown checkboxes, remains authority.


## Milestone F — Immediate Rebaseline Bridge + Recovery architecture

Immediate gate before any more coding:
- freeze J-177F before T-ORCHESTRATION;
- classify/adopt work completed through T-EXECUTION;
- preserve valid source/evidence subject to applicability review;
- create a replacement remaining-work Job/graph under migrated governance;
- do not manually rewrite the IN_PROGRESS J-177F record.

After that operational bridge, implement the future runtime automation below.

Deliverables:
- project-first resolver;
- WorkCursor + ResumeDescriptor;
- SourceTargetSet + plan/hash/graph binding;
- START/RESUME/RECOVER resolvers for Planning/Coding/Tester;
- crash-consistent durable transition identity;
- parent/child Run reconciliation;
- machine-readable GateDiagnostic;
- workflow replay policy + deterministic verification readiness;
- MutationIntent write-ahead;
- lease fencing token;
- state-schema migration;
- Human Rebaseline/Salvage;
- AcceptedIntegrationBaseline.

Gate:
- crash or intentional Human architecture change can be reconciled without conversation history, blanket revert, duplicate non-idempotent action or stale authority.

## Milestone G — UI execution-mode architecture

Deliverables:
- Designer / UX Coder / UI Coder role contracts;
- UI_FIRST / UX_FIRST product workflow classification;
- ui_execution_mode = BROWSER_FIRST | DESIGN_FIRST;
- UX execution contract;
- visual_authority_source = existing approved shell/design system, Designer spec, or Human direction;
- BROWSER_FIRST may still invoke Designer for unresolved material visual decisions without invoking Penpot;
- BROWSER_FIRST default for any existing product shell;
- explicit Human approval before discarding/rebuilding/replacing any existing shell through DESIGN_FIRST; aesthetic dissatisfaction alone is not a mode-switch authority;
- greenfield risk-based mode selection rather than automatic Penpot;
- asset routing as a signal only;
- mode-specific design readiness rules.

Gate:
- Planner can select the lightest safe UI execution mode without UI Coder inventing material product/design decisions.

## Milestone H — agent-browser shared runtime + optional Penpot

Primary Local UI surface:
- Vercel agent-browser is the default runnable-source observation loop for UI Coder;
- Tester uses a separate independent agent-browser session for verification.

agent-browser deliverables:
- Local PROBE of navigation/snapshot/interact/re-snapshot/viewport/screenshot/diff/console/error/cleanup;
- separate UI Coder development and Tester verification operation identities;
- Tester browser workspace;
- direct Human screenshot paths;
- exact target/viewport applicability.

Optional DESIGN_FIRST/Penpot deliverables (activated only by a genuine approved need, never to manufacture a redesign for testing):
- Penpot is activated only when the selected mode requires pre-code visual architecture;
- generic UI Coder + Penpot capability PASS may be reused through UPSTREAM_OBSERVED_REF or HUMAN_ACCEPTED_EXTERNAL planning premise;
- Local Penpot invocation/readback/resume/artifact integration remains OBSERVED locally when used;
- immutable PenpotEvidenceBundle/read-only inspection path for non-browser design checkpoints, independent of agent-browser readiness when the Penpot target is not browser-inspectable;
- UI DESIGN APPROVED baseline when required.

Gate:
- BROWSER_FIRST can proceed without Penpot;
- DESIGN_FIRST can resume/inspect its Penpot artifact when Penpot is selected;
- UI Coder browser observations never count as Tester PASS.

## Milestone I — mode-specific flow to SHELL_READY

BROWSER_FIRST:
- resolve visual authority first; use existing approved shell/design system/Human direction when sufficient, otherwise Designer supplies visual direction/spec without Penpot;
- UI Coder edits source directly;
- agent-browser drives development/refinement feedback;
- Coder self-check + Reviewer;
- Tester independently verifies runnable UI;
- Human reviews evidence when required.

DESIGN_FIRST:
- Designer/UI Coder Penpot design;
- objective design evidence + Human approval;
- UI DESIGN APPROVED;
- source implementation;
- agent-browser development loop;
- Reviewer;
- Tester independent runtime verification.

Shared deliverables:
- representative shell pilot when risk justifies it;
- SHELL_READY based on runtime evidence;
- UX_FIRST UI Update Pack routing through BROWSER_FIRST or DESIGN_FIRST;
- Rebaseline/Salvage when Human decides an existing shell must be discarded/rebuilt; regression coverage uses a genuine approved rebuild or controlled fixture, never a manufactured product redesign.

Gate:
- no broad scale-out from an unverified shell where SHELL_READY is required;
- no Penpot requirement for safe source-first UI iteration;
- existing-shell rebuild uses DESIGN_FIRST only after Human approval.

## Milestone dependency chain

```text
F immediate rebaseline + recovery foundations
  -> G UI execution-mode architecture
  -> H agent-browser shared runtime

G selects BROWSER_FIRST
  -> I browser-first source flow

G selects DESIGN_FIRST
  -> H optional Penpot integration
  -> I design-first flow
```

No later milestone may infer readiness from numbering alone; the selected mode and its predecessor gates must be satisfied explicitly.
