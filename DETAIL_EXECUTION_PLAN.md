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

## UI execution policy review gate — UNDER REVIEW / CR REQUIRED

Status for Milestones G–I: **UNDER REVIEW**.

This UI execution-mode architecture is intentionally provisional. It may be implemented as accepted Local architecture only after a **Human-invoked CR** independently reviews the exact plan revision, makes any required corrections, and explicitly clears the mark.

Rules:
- normal Reviewer PASS cannot remove this mark;
- no automatic workflow may invoke CR to clear it;
- CR remains stateless/fresh and uses durable source/plan evidence;
- removing the mark requires explicit CR disposition on the Local-specific environment-aware policy;
- until then, Milestones G–I must not become irreversible mandatory UI workflow authority.

Environment premise under review:
- Local has a stronger Vercel agent-browser development/inspection surface than GSA Online;
- therefore Local may prefer BROWSER_FIRST more often;
- Online may rationally prefer a heavier pre-code visual-design workflow while browser capability is weaker;
- this difference is valid only if shared role authority, Tester independence, evidence semantics and Human visual acceptance remain common;
- Planner must route from observed runtime capability plus design risk, not from deployment label alone.

## Milestone G — UI visual-authority + dedicated UI Coder workflow

Deliverables:
- Designer / UX Coder / UI Coder role contracts;
- UI_FIRST / UX_FIRST product classification;
- visual_authority_source contract;
- existing-shell preservation by default;
- explicit Human approval plus Rebaseline/Salvage before shell discard/rebuild/replacement;
- greenfield/rebuild source-first execution;
- UI Coder state machine: UI_GROUND -> UI_IMPLEMENT -> UI_RENDER -> UI_INSPECT -> UI_REFINE -> UI_SELF_CHECK;
- dedicated UI workspace for references/screenshots/comparisons/assets/prompts/tokens/notes;
- UI workspace is non-runtime; promoted production assets use canonical product paths with provenance/approval.

Gate:
- UI Coder can implement/refine real UI without inventing unresolved visual decisions and without a parallel mockup authority.

## Milestone H — agent-browser shared runtime

Deliverables:
- Local agent-browser capability PROBE;
- UI Coder development session;
- independent Tester verification session;
- screenshot/snapshot/diff/viewport/interaction evidence;
- direct Human evidence paths;
- exact target/browser-operation applicability;
- session/evidence authority isolation.

Gate:
- UI Coder can observe/refine the real app and Tester can independently verify it without sharing PASS authority.

## Milestone I — UI acceptance / shell readiness

Normal UI:
- source/browser loop;
- UI_SELF_CHECK;
- Reviewer;
- Tester;
- Human when subjective acceptance is required;
- UI_ACCEPTED.

Greenfield or Human-approved shell rebuild:
- Designer/Human visual direction where needed;
- real-source shell build;
- agent-browser refine loop;
- representative-content/responsive pilot;
- Reviewer;
- Tester;
- Human checkpoint where required;
- SHELL_READY.

Gate:
- no existing shell is discarded without Human approval plus completed Rebaseline/Salvage;
- no shell baseline is accepted from mockup-only evidence;
- SHELL_READY requires runnable-source evidence.

## Milestone J — extensible UI resources / skills

Optional extensions:
- icon/font/component libraries;
- open/stock assets;
- image generation;
- SVG/vector tooling;
- asset optimization;
- responsive/accessibility/design-token/framework/visual-regression/asset-integration skills.

Gate:
- core UI workflow still works with source + agent-browser alone;
- optional provider absence does not silently block execution;
- imported/generated durable assets preserve provenance when relevant.

## Milestone K — UI workflow calibration

Pilots:
- brownfield existing shell;
- simple greenfield shell;
- Human-approved rebuild or controlled fixture;
- UX_FIRST Update Pack;
- interruption/resume across UI workflow states;
- UI Coder vs Tester session isolation;
- optional-provider absence.

Gate:
- workflow becomes mandatory only after real Local evidence shows the browser-driven UI loop is resumable, independently verifiable and bounded.

## Milestone dependency chain

```text
F immediate rebaseline + recovery foundations
  -> G UI authority + UI Coder workflow
  -> H agent-browser shared runtime
  -> I UI acceptance / SHELL_READY
  -> K workflow calibration

G
  -> J optional UI providers/skills
```

No later milestone may infer readiness from numbering alone; predecessor gates must be satisfied explicitly.
