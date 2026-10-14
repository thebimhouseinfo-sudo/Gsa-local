# Side Plan — Execution Graph Evidence Order & In-Place Recovery (GSA Local)

> **STATUS:** SIDE PLAN / PLANNER SELF-RL PASS / **AWAITING HUMAN APPROVAL FOR IMPLEMENTATION**.
> **Source of truth:** GitHub `main`; this document is not a replacement for `IMPLEMENTATION_PLAN.md`, `DETAIL_EXECUTION_PLAN.md`, or the active UAR work.
> **Origin:** isolated real Mac test `~/Projects/GSA-Stock-QA`, 2026-10-14. **No product/source/SQLite mutation is authorized by this document.**
> **Scope:** fix the GSA Local workflow, not FEFO/atomicity/idempotency business logic.

## GOAL — giữ workflow đúng và tiếp tục từ bằng chứng thật

Prevent any `READY` Execution Graph that requires a Tester evidence output **before** the graph can reach its declared production boundary. For a graph already registered with this topology defect, provide a **Human-approved, independently reviewed, bounded graph-contract correction** that preserves the existing J1 source/change set, completed reviews, verification, Local CR, original approval lineage, and event audit. Resume deterministically from the exact pending gate: **J1 DONE → Tester OBSERVED output → J2 ACTIVE**. Reject impossible or unauthorized corrections instead of bypassing Tester or replaying J1.

### Non-goals / hard safety boundaries

- **Not** a complete new planning subsystem, graph-version migration engine, generic replan, UI overhaul, broad Tester redesign, or replacement of existing UAR phases.
- Never synthesize `OBSERVED` evidence, mark TEST PASS without execution, weaken CR, insert J2 `ACTIVE` early, reset `.gsa/state/gsa.db`, replay J1 coding, or silently supersede its exact artifacts.
- No direct SQLite edits by the user as a production solution. Read-only SQL is permitted for diagnosis.
- A narrow topology correction is **not** permission to alter requirements, business goals, code, Job Packs, acceptance criteria, evidence names/modes, or previously executed Tester results.
- Human approval to *write code* remains a separate gate after this side plan is reviewed.

## Grounded findings / current source

1. Human's SQLite query of `graph_version=1` showed `jp-j1-fefo-atomicity` ACTIVE and `jp-j2-idempotency` PENDING, both in `m1-reservation-correctness`. `chk-j1-verify` uses `MILESTONE_GATE`, with prerequisite `J1 DONE`; output `ev-j1-verify` is REQUIRED by J2.
2. `src/registry.rs::tester_boundary_state_tx`: `MILESTONE_GATE` is reached only at milestone `VERIFY`. Milestone `VERIFY` follows completion of **all** Job Packs in that milestone (`complete_active_jobpack`). Thus the constraint cycle is `J2.start -> J2.done -> milestone.VERIFY -> chk-j1-verify.OBSERVED -> J2.start`.
3. `src/execution_graph.rs::validate_checkpoints` verifies IDs, boundaries, prerequisite existence, milestone orders, and a partial augmented Job Pack dependency graph, but **does not** model the event `milestone.VERIFY` or this cycle. `src/workflow.rs::validate_job_builder_submission` and `Registry::register_execution_graph` both call graph validation; the missing rule is in the common validator.
4. `Registry::register_execution_graph` creates a new graph version, initializes all Job Packs PENDING, and retires ACTIVE work in the prior CURRENT graph. `CodeReview`, `Verification`, `LocalCR`, Tester records and WorkCursor are keyed by graph version/change set or by exact target. A normal re-registration **cannot** be called a safe continuation of J1.
5. `begin_or_resume_planning_run` rejects new planning when stage `REGISTERED`; current routing has no Human-reviewed repair path for this registered graph.
6. Human Mac J1 change set `a7dabdfaa88c700e0f270c8f327b43d913535ce0bd9588cdde62117a56b65753` produced `CODE_REVIEW_PASS`, `VERIFICATION_RESULT=TEST_PASS`, `CODE_CR_PASS`; terminal transition then failed with `PLAN_GAP`. Status remains J1 ACTIVE because earlier terminal transaction rolled back. **These passes are not a Tester checkpoint result**.
7. The latest `main` terminal-transition repair can protect J1 DONE against premature J2 activation. It **cannot** make a structurally impossible `MILESTONE_GATE` become reachable. Retain its fail-closed guard; do not treat it as the final fix.

## Architecture decision (bounded side path)

**Choose a same-`graph_version`, append-only, Human-approved `GraphContractAmendment` for topology-only corrections**, rather than ordinary `register_execution_graph` and wholesale data adoption.

- The original approved PlanArtifact `plan_revision/plan_hash` and original graph definition remain auditable; keep a **separate** monotonic `graph_contract_revision` and `effective_graph_hash` for approved amendments. The original effective graph snapshot/diff stays in an append-only amendment ledger.
- Each correction binds to `graph_version`, original plan revision/hash, exact previous effective graph hash, exact checkpoint ID and original definition hash. Permit compare-and-swap once inside an execution-lease-owned SQLite transaction. A replay of the same accepted amendment is idempotent; conflicting/stale replays fail closed.
- Applied projection changes may update `execution_test_checkpoints.boundary` and `definition_json` (plus prerequisite/index tables **only when actually changed**). The immutable ledger must retain before/after snapshots and approvals, and every resolver must see the same effective contract. Do not merely overwrite a stored row with no provenance.
- **Stock-QA allowlist:** only `chk-j1-verify: MILESTONE_GATE → AFTER_JOBPACK_SET` with unchanged `J1 DONE` prerequisite, checkpoint ID, milestone ID, output `ev-j1-verify`, modes, requirements and consumers. Any wider modification requires a new reviewed plan and an explicit migration/reconciliation design **outside this side plan**.
- Proposal is allowed only when the active graph and exact source/change-set lineage are intact, J2 has not started, and no Tester attempt/output is bound to the checkpoint being changed. If any precondition fails: `GRAPH_REPAIR_BLOCKED`; no partial mutation.
- **Approval sequence:** detect `GRAPH_ORDER_GAP` -> form minimal diff/proof -> independent Reviewer/Local CR check of contract-only change -> present exact diff/effect to Human -> affirmative Human approval -> atomic apply -> record amendment event -> normal runtime resume. A model's suggestion or CI PASS alone is not Human approval.
- The changed topology must still satisfy `ExecutionGraph::validate_against_plan` and the temporal validator; goals, plan intent and scope must be unchanged. If not, fail into a separate replan decision, **not** an implicit amendment.
- Existing J1 `code_workflow_state`, `code_checkpoints`, `code_reviews`, `code_cr_reviews`, `verification_runs`, mutation journal, source bytes, and Plan binding remain untouched. Validate that the original exact CR evidence fingerprint is still applicable; if not, route only the affected review gate for independent revalidation, **not** Coder J1 redo.

## Phase S0 — Source/evidence freeze and executable reproduction

**GOAL:** establish a read-only, exact baseline and prove the graph cycle before changes.

Implementation steps:
1. Record Git `main` SHA, isolated fixture `graph_version/plan_revision/plan_hash`, J1 change-set hashes and CR/verification IDs; read-only snapshot/backup with WAL-aware SQLite API or SQLite `.backup` when a consistent copy is necessary.
2. Reproduce the exact Stock-QA topology in a fresh in-memory/temp registry: J1/J2 same milestone, `MILESTONE_GATE` with J1 DONE, mandatory output for J2.
3. Check the relevant existing tests: `tests/controller.rs`, `tests/job_builder.rs`, `tests/tester_evidence.rs`, `tests/tester_orchestration.rs` and source in `src/execution_graph.rs`, `src/registry.rs`, `src/controller.rs`, `src/workflow.rs`.
4. Document initial code/runtime invariant and prove regression RED before changing production logic.

**Gate S0:** reproducible failure and saved evidence; no original fixture DB/source mutation; source-vs-registry identities reconciled.

## Phase S1 — Temporal Graph Validator + Job Builder feedback

**GOAL:** impossible graph is rejected **before registration**, with an actionable structural cycle trace, while legitimate graphs remain valid.

Implementation steps:
1. Extend `ExecutionGraph::validate` with a deterministic **event precedence DAG** (no model reasoning). Nodes include `JP.start`, `JP.review_pass`, `JP.done`, `milestone.VERIFY`, and `checkpoint.OBSERVED`; edges include:
   - each Job Pack `start -> review_pass -> done`;
   - `depends_on: predecessor.done -> successor.start`;
   - `AFTER_JOBPACK_SET` prerequisites at their declared `REVIEW_PASS/DONE` -> checkpoint;
   - `BEFORE_JOBPACK` prerequisite events -> checkpoint -> gated Job Pack start;
   - `MILESTONE_GATE`: **every** Job Pack in that milestone `done -> milestone.VERIFY -> checkpoint`;
   - every REQUIRED EvidenceRequirement `checkpoint.OBSERVED -> consumer.start`;
   - sequential milestones only after the prior milestone's completion/checkpoint gate, where applicable.
2. Detect a cycle by topological sort/DFS; report `GRAPH_UNSATISFIABLE_EVIDENCE_ORDER` and explicit IDs/events, e.g. `jp-j2.start → jp-j2.done → m1.VERIFY → chk-j1-verify.OBSERVED → jp-j2.start`. Keep deterministic ordering and readable evidence provenance. Optional evidence must not create a hard mandatory edge unless another declared gate requires it.
3. Preserve existing graph validation and reject before `approve_current_plan` / `register_execution_graph` writes whenever Job Builder returns an invalid READY graph. A bounded Job Builder correction can retry **only the declared topology**; it must return `PLAN_GAP` if approved-plan information is insufficient. No fabricated checkpoint, no new jobpack, no silent scope enlargement.
4. Update `harnesses/job_builder.md` with an explicit boundary timing rule. Runtime validation remains authoritative, not the prompt.
5. Add negative and positive tests for `MILESTONE_GATE` same-milestone consumer cycle; valid `AFTER_JOBPACK_SET J1 DONE -> J2`; valid `BEFORE_JOBPACK`; cross-milestone gate; independent Job Packs; pre-existing ordinary depends_on cycles; optional outputs and invalid unknown references. Require exact machine-checkable rejection.

**Gate S1:** old failing graph rejected at both Job Builder submission and DB registration; legitimate predecessors still register; Rust CI green. No existing graph auto-mutated.

## Phase S2 — Human-reviewed registered-graph amendment

**GOAL:** resolve **only** the approved checkpoint timing defect of a CURRENT registered graph without losing or rebinding J1 work.

Implementation steps:
1. Add a separate `GRAPH_REPAIR_REQUIRED` workflow/resume state (not a fake Coder/Planner stage) with exact graph/revision/fingerprint and `pending_amendment_id`; surfaced on startup and after `PLAN_GAP_EVIDENCE_BEFORE_ACTIVATION`. Read-only diagnosis must be possible without mutation.
2. Generate a minimal, typed `GraphContractAmendment` proposal with original graph/checkpoint fingerprint, target IDs, single allowed boundary change, rationale and before/after temporal cycle report. Distinguish a topology error from genuine unavailable/stale Tester evidence; only the former may propose amendment.
3. Independent Reviewer and Local CR review the graph-only diff and preservation proof. Obtain **explicit Human affirmative approval** for the exact amendment fingerprint (no auto approval after RL/CR).
4. Implement append-only amendment history and transactionally update its effective projection under lease + compare-and-swap; retain original snapshot, before/after hashes, actor/approvals/events. Enforce graph contract scope and no Tester outputs/attempts for amended checkpoint. Reject stale cursor, modified source, wrong project, wrong plan, wrong graph version, already active J2, duplicate conflicting amendment.
5. Reconcile WorkCursor and existing CR target without reinitializing code state. The change remains on the same graph version; J1 code and evidence records remain physically keyed as before. Make both resumption and error messages point to the **next declared gate**, not to Coder J1.
6. On failed validation/approval/write, leave the original current graph and all artifacts unchanged; report `GRAPH_REPAIR_BLOCKED` with a precise reason and Human-safe next action.

**Gate S2:** after a successful approved amendment, exact J1 source hashes + `change_set_id` + PASS records unchanged; original graph contract and approval history readable; amendment revision monotonic; crash/restart yields one effective contract, no duplicated repair or source writes.

## Phase S3 — Terminal handoff and Tester evidence enforcement

**GOAL:** resume the persisted J1 terminal outcome and execute the declared Tester checkpoint before enabling J2.

Implementation steps:
1. From the existing `REVIEW_PASS + TEST_PASS + CODE_CR_PASS` lineage, resume Local CR/terminalization idempotently. Reuse exact PASS only when its persisted evidence fingerprint matches; do **not** ask Coder to reimplement J1.
2. `complete_active_jobpack` commits J1 `DONE` separately from eligibility of J2. Resolve checkpoint `chk-j1-verify` at `AFTER_JOBPACK_SET` with prerequisite J1 `DONE`.
3. Tester runs its own real verification procedure, persists only `OBSERVED` result `ev-j1-verify` on the matching target; on FAIL/NEEDS_HUMAN/INTEGRATION_NOT_READY, stop with correct classification. `VERIFICATION_RESULT=TEST_PASS` from Coder is **not a substitute** for this evidence.
4. `activate_eligible_jobpack_tx` may activate `jp-j2-idempotency` only after evidence is present, compatible, observed, not stale, and all dependencies hold. Avoid fake `PLAN_GAP` caused by checking J2 prematurely; a permanently invalid topology is reported as structural spec gap.
5. Ensure restart between every transition takes the exact persisted next step, with at-most-once durable gate transitions; no duplicate Local CR, Tester outputs or JobPack DONE events.

**Gate S3:** `J1 DONE → Tester DUE → Tester PASS with OBSERVED output → J2 ACTIVE` in that order; failure branches fail closed; unchanged J1 source and change set.

## Phase S4 — Regression matrix, independent review and real Mac acceptance

**GOAL:** prove prevention and recovery in a real retained project, not only through green synthetic unit tests.

- Automated tests: S1 negative graph rejection, S2 valid/invalid amendment, S3 happy path, crash after amendment, crash after J1 DONE, checkpoint retest, stale/mutated source, reused evidence with wrong fingerprint, altered plan binding, lease mismatch, wrong workspace, J2 already started, pre-existing Tester attempt, conflicting repair, no duplicate event, independent Job Packs.
- CI: `cargo fmt --check`, `cargo test`, repository `scripts/ci.sh`; exact-SHA success; independent source code CR at mature checkpoint. QA simulation must not masquerade as human real-Mac acceptance.
- **Mac Human acceptance only after source CI+CR:** from `~/Projects/GSA-Stock-QA`, preserve original `.gsa/state/gsa.db` and fixture source; use the supported CLI graph repair and explicit Human approval. Verify persisted J1 change set `a7dabdfaa88c700e0f270c8f327b43d913535ce0bd9588cdde62117a56b65753` remains exact, and the ordered J1→Tester→J2 logs.
- Then run real J2 coding/review/verification/Local CR through GSA. Independently run the hidden external fixture oracle `bash ~/Downloads/gsa-stock-qa-kit/check_acceptance.sh ~/Projects/GSA-Stock-QA` as product acceptance (if the original kit remains available). Existing smoke green tests are never treated as this oracle.
- If hidden oracle fails, report product defect, do not auto-pass or incorrectly reopen graph repair.
- Finish only after real evidence read-back and latest `main` merge/CI; otherwise mark `HUMAN_TEST_REQUIRED` with a specific blocker.

**Gate S4:** observed complete workflow with no `PLAN_GAP`, no rework of J1, exact artifact provenance and independent oracle results; CI green alone is insufficient.

## Acceptance matrix (minimum)

| Case | Expected behavior |
|---|---|
| Same milestone J2 requires `MILESTONE_GATE` output | reject `GRAPH_UNSATISFIABLE_EVIDENCE_ORDER` with cycle path |
| J1 DONE → `AFTER_JOBPACK_SET` Tester → J2 | valid, Tester executes before J2 |
| Before-jobpack + after-jobpack checkpoints | valid only if event topology acyclic |
| Cross-milestone `MILESTONE_GATE` | allow when producer milestone can VERIFY/COMPLETE before consumer starts |
| Later independent Job Pack has unrelated evidence requirement | no premature blockage of preceding eligible pack |
| Already registered bad graph, no approval | no mutation; blocked with exact requested amendment |
| Approved one-field correction | original graph + amendment ledger preserved; same graph version and all J1 records unchanged |
| Stale graph hash, changed source, wrong lease, wrong checkpoint or J2 already ACTIVE | fail closed, transaction rollback |
| Failed/replayed/crashed amendment | zero partial writes, idempotent exact replay only |
| J1 reviewed code and CR PASS but Tester output absent | do not activate J2; do not fabricate evidence |
| Tester result FAIL / NEEDS_HUMAN / stale | correct block/repair decision; no false TEST_PASS |
| Restart after J1 DONE or Tester OBSERVED | deterministic resume; no duplicate gate/event |
| Stock-QA external acceptance oracle | genuine independent final check, not replaced by prior three green smoke tests |

## Sequencing and authorization

`S0 → S1 → S2 → S3 → S4`. Do not parallelize S1 validator contract changes with a live S2 DB amendment. Keep one `main` source of truth; no stale experimental branch pileup. Code changes begin **only on explicit Human implementation approval**. Plan modification proposals require explicit approval when they expand scope. Work within GSA Local source; keep the Stock-QA fixture isolated. Existing runtime patches are subject to this validator and test suite, but are not deleted on speculation.

## Planner self-RL record (not an independent CR verdict)

- **RL1 — target/goal drift:** removed the tempting "register graph v2 and copy J1 PASS rows" shortcut because current schema binds records to graph version and ordinary registration retires active work. Added same-graph, append-only, fingerprinted amendment and a prohibition on silent rebinding.
- **RL2 — overbroad validator:** added event-level cycle checks instead of banning all same-milestone evidence dependencies. Kept positive cases for `AFTER_JOBPACK_SET`, `BEFORE_JOBPACK`, independent packs and cross-milestone gates.
- **RL3 — false green:** separated Coder `TEST_PASS` from Tester-owned `OBSERVED`, and CI regression from real Mac/oracle acceptance. Added fail-closed paths and crash/idempotence tests.
- **RL4 — approval/contract drift:** restricted registered graph correction to one checkpoint timing field; added independent reviewer and explicit Human approval, retained plan hash/goal/phase-goal invariants and deferred generic graph-version migration.
- **Self-review result:** PASS for a **side-plan design** against the supplied Stock-QA failure and the current code contracts. **Not** a claim that implementation, independent CR, CI, or Human acceptance has passed.

## Source references

- `src/execution_graph.rs`: `ExecutionGraph::validate`, `validate_checkpoints`, `validate_against_plan`, `CheckpointBoundaryKind`.
- `src/registry.rs`: `tester_boundary_state_tx`, `complete_active_jobpack`, `resolve_or_activate_work`, `register_execution_graph`, `approve_current_plan`, `current_graph_binding_tx`, `record_tester_attempt_evidence`.
- `src/workflow.rs`: `validate_job_builder_submission`, Job Builder PLAN_GAP and Reviewer/Local CR loops.
- `src/controller.rs`, `src/app.rs`: resume dispatcher, CR terminal handoff and Tester resolution.
- `tests/controller.rs`, `tests/job_builder.rs`, `tests/tester_evidence.rs`, `tests/tester_orchestration.rs`.
- `DETAIL_EXECUTION_PLAN.md`: rebaselined Tester checkpoint architecture; `WORKFLOW_LESSONS_LEARNED.md`: resume identity, no implicit restart, false-green evidence, fail-closed recovery.
- Real Mac SQLite observation 2026-10-14 and CI regressions documented in `.gsa/PROJECT_STATUS.md`.
