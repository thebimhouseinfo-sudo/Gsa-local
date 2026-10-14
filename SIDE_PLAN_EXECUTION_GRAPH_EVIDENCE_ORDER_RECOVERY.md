# Side Plan — Execution Graph Evidence Order & In-Place Recovery (GSA Local)

> **STATUS:** SIDE PLAN / PLANNER RL AFTER SOURCE REVIEW **PASS** / **INDEPENDENT CR RECHECK REQUIRED** / **AWAITING HUMAN APPROVAL FOR IMPLEMENTATION**.
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
8. `activate_eligible_jobpack_tx` chooses the first dependency-ready Job Pack in lexicographical ID order. `validate_checkpoints` only adds Tester prerequisite edges in an **in-memory validation graph**, not to persisted `execution_jobpack_dependencies`. An acyclic graph can therefore still select a consumer before its evidence producer if names sort differently. Validator and scheduler must agree on evidence precedence.
9. `code_cr_boundary_work_tx` builds its CR boundary identity from verification run and already-resolved Tester evidence. After changing an untriggered DONE-prerequisite checkpoint, the terminal fingerprint **may** remain the same, but this must be proven by before/after checks rather than assumed.
10. `MilestoneController::resolve_next` can repair a Tester PRODUCT_FAILURE only with exactly one `REVIEW_PASS` prerequisite bound to the ACTIVE Job Pack. A `DONE` prerequisite is not auto-repairable through this path; recovery policy must not silently reopen J1 or label the product PASS.

## Architecture decision (bounded side path)

**Choose a same-`graph_version`, append-only, Human-approved `GraphContractAmendment` for topology-only corrections**, rather than ordinary `register_execution_graph` and wholesale data adoption.

- The original approved PlanArtifact `plan_revision/plan_hash` and original graph definition remain auditable; keep a **separate** monotonic `graph_contract_revision` and `effective_graph_hash` for approved amendments. The original effective graph snapshot/diff stays in an append-only amendment ledger. Existing registries (including Stock-QA graph v1) do **not** already have these fields: derive and persist an initial canonical graph snapshot/hash deterministically from the current normalized tables, verify their consistency, and test the migration on a disposable DB copy before enabling amendment. If reconstruction is incomplete or inconsistent, fail closed with a read-only diagnostic; do not guess an original baseline.
- Each correction binds to `graph_version`, original plan revision/hash, exact previous effective graph hash, exact checkpoint ID and original definition hash. Permit compare-and-swap once inside an execution-lease-owned SQLite transaction. A replay of the same accepted amendment is idempotent; conflicting/stale replays fail closed. Define an unambiguous canonical serialization of the **effective** graph, stable across restart and DB row ordering; the audit ledger must contain the original definition as well as before/after hashes.
- Applied projection changes may update `execution_test_checkpoints.boundary` and `definition_json` (plus prerequisite/index tables **only when actually changed**). The immutable ledger must retain before/after snapshots and approvals, and every resolver must see the same effective contract. Do not merely overwrite a stored row with no provenance.
- **Stock-QA allowlist:** only `chk-j1-verify: MILESTONE_GATE → AFTER_JOBPACK_SET` with unchanged `J1 DONE` prerequisite, checkpoint ID, milestone ID, output `ev-j1-verify`, modes, requirements and consumers. Any wider modification requires a new reviewed plan and an explicit migration/reconciliation design **outside this side plan**.
- Proposal is allowed only when the active graph and exact source/change-set lineage are intact, J2 has not started, and no Tester attempt/output is bound to the checkpoint being changed. If any precondition fails: `GRAPH_REPAIR_BLOCKED`; no partial mutation.
- **Approval sequence:** detect `GRAPH_ORDER_GAP` -> form minimal diff/proof -> independent Reviewer/Local CR check of contract-only change -> present exact diff/effect to Human -> affirmative Human approval -> atomic apply -> record amendment event -> normal runtime resume. A model's suggestion or CI PASS alone is not Human approval.
- The changed topology must still satisfy `ExecutionGraph::validate_against_plan` and the temporal validator; goals, plan intent and scope must be unchanged. If not, fail into a separate replan decision, **not** an implicit amendment.
- Existing J1 `code_workflow_state`, `code_checkpoints`, `code_reviews`, `code_cr_reviews`, `verification_runs`, mutation journal, source bytes, and Plan binding remain untouched. Explicitly compute `code_cr_boundary_work_tx` before/after to verify the same `JOBPACK_TERMINAL` boundary IDs, verification-run identity and exact CR evidence fingerprint. If any differ, retain existing PASS history but **do not reuse it as current authority**; route only the affected independent review gate for revalidation, **not** Coder J1 redo. Keep this reconciliation durable across restart.

## Phase S0 — Source/evidence freeze and executable reproduction

**GOAL:** establish a read-only, exact baseline and prove the graph cycle before changes.

Implementation steps:
1. Record Git `main` SHA, isolated fixture `graph_version/plan_revision/plan_hash`, J1 change-set hashes and CR/verification IDs; read-only snapshot/backup with WAL-aware SQLite API or SQLite `.backup` when a consistent copy is necessary.
2. Reproduce the exact Stock-QA topology in a fresh in-memory/temp registry: J1/J2 same milestone, `MILESTONE_GATE` with J1 DONE, mandatory output for J2.
3. Check the relevant existing tests: `tests/controller.rs`, `tests/job_builder.rs`, `tests/tester_evidence.rs`, `tests/tester_orchestration.rs` and source in `src/execution_graph.rs`, `src/registry.rs`, `src/controller.rs`, `src/workflow.rs`.
4. Document initial code/runtime invariant and prove regression RED before changing production logic.
5. Inspect the actual deterministic candidate order and existing effective graph storage: verify whether any required-evidence edge is absent from persisted scheduler dependencies and whether the legacy normalized tables reconstruct into an unambiguous baseline.

**Gate S0:** reproducible failure and saved evidence; no original fixture DB/source mutation; source-vs-registry identities reconciled.

## Phase S1 — Temporal Graph Validator + Job Builder feedback

**GOAL:** impossible graph is rejected **before registration**, with an actionable structural cycle trace, while legitimate graphs remain valid.

Implementation steps:
1. Extend `ExecutionGraph::validate` with a deterministic **event precedence DAG** (no model reasoning). Include `JP.start`, `JP.review_pass`, `JP.done`, `milestone.VERIFY`, `milestone.COMPLETE`, and `checkpoint.OBSERVED`. Edges must reflect **actual runtime boundaries**:
   - per-Job Pack `start -> review_pass -> done` and `depends_on: predecessor.done -> successor.start`;
   - each checkpoint prerequisite `JP.review_pass` or `JP.done` according to its exact `PrerequisiteState`, not an inferred DONE;
   - `AFTER_JOBPACK_SET`: declared prerequisite events -> checkpoint; it can be reached after `REVIEW_PASS` even while the referenced Job Pack is ACTIVE;
   - `BEFORE_JOBPACK`: prerequisite events -> checkpoint -> gated Job Pack start;
   - `MILESTONE_GATE`: every Job Pack in the milestone `done -> milestone.VERIFY -> checkpoint`;
   - checkpoint obligations for milestone completion -> `milestone.COMPLETE` (avoid claiming completion before its Tester gates);
   - each **REQUIRED** EvidenceRequirement `checkpoint.OBSERVED -> consumer.start`;
   - milestone precedence: previous `milestone.COMPLETE ->` next milestone's Job Pack start.
2. Detect cycles by stable topological ordering/DFS, report `GRAPH_UNSATISFIABLE_EVIDENCE_ORDER` with exact IDs/events and actionable failure, e.g. `jp-j2.start → jp-j2.done → m1.VERIFY → chk-j1-verify.OBSERVED → jp-j2.start`. Static validation establishes **possibility of execution**, not that the Tester runtime/capability will pass. Optional evidence is not a mandatory start edge unless a separately declared gate makes it mandatory.
3. **Make scheduler eligibility agree with this DAG.** Currently graph validation infers evidence-prerequisite edges, but `activate_eligible_jobpack_tx` selects lexicographically from **persisted** explicit `depends_on`. Implement the least disruptive deterministic solution (derive evidence producer constraints at eligibility, or persist equivalent scheduler edges) and test both `J1 sorts before J2` and `J2 sorts before J1` IDs. No evidence consumer may become ACTIVE before its producer checkpoint is satisfiable; do not deadlock independent eligible work just because another successor lacks evidence. A missing output from a reachable checkpoint is `WAIT_FOR_TESTER`, **not** proof of an invalid graph. A cycle is `GRAPH_ORDER_GAP`.
4. Apply the common temporal validator both to Job Builder READY submission and to Registry graph registration. Invalid READY triggers the existing **bounded** Job Builder validation-repair path; when the approved plan cannot support the minimal correction, return explicit `PLAN_GAP` rather than changing plan scope or fabricating an evidence source. Reject before calling plan/graph approval mutations.
5. Update `harnesses/job_builder.md` with boundary/event timing constraints as guidance; deterministic runtime contract remains authoritative.
6. Add negative and positive tests: same-milestone `MILESTONE_GATE` consumer cycle, legal `AFTER_JOBPACK_SET` J1→J2, legal `BEFORE_JOBPACK`, legal preceding-milestone gate, independent Job Packs, reversed name ordering, ordinary `depends_on` cycles, optional output, invalid reference, and checkpoint `REVIEW_PASS` prerequisite that legitimately precedes J1 DONE. Require machine-readable cycle proof and no false negatives.

**Gate S1:** old failing graph rejected at both Job Builder submission and DB registration; legitimate predecessors still register; Rust CI green. No existing graph auto-mutated.

## Phase S2 — Human-reviewed registered-graph amendment

**GOAL:** resolve **only** the approved checkpoint timing defect of a CURRENT registered graph without losing or rebinding J1 work.

Implementation steps:
1. Introduce a **separate**, durable `GRAPH_REPAIR_REQUIRED` cursor, not a fake Planning stage or a generic replan. Include exact graph version, approved plan revision/hash, effective graph hash, impacted checkpoint, expected source/change-set, and proposed amendment identity. Route the next action via startup/dispatcher and report `GRAPH_ORDER_GAP` distinctly from temporarily missing/stale Tester outputs. No silent repair from a Coder instruction.
2. Add legacy-aware **baseline enrollment**: reconstruct graph v1 canonically from normalized Registry tables, validate foreign keys/projections, save immutable pre-amendment snapshot/hash and initialize contract revision 0. Perform safe migration under SQLite transaction without rewriting existing code, CR, verification, evidence, or pointers. Abort on mismatches. On a fresh graph, record the canonical initial snapshot at registration.
3. Produce a typed one-field `GraphContractAmendment` proposal for `chk-j1-verify` and its exact `MILESTONE_GATE -> AFTER_JOBPACK_SET` change; include before/after event-order proof, baseline hash, exact affected row and proof that the declared J1 DONE prerequisite/output/consumer remain unchanged. Check already-recorded Tester attempts (including non-final attempts), prepared/in-flight Tester execution, CR boundary and code-change lineage before proposing.
4. Independent Reviewer and Local CR review the contract-only diff and preservation proof. Obtain explicit affirmative Human approval tied to the displayed proposal fingerprint **after** review. Different proposal/fingerprint, source drift, wrong workspace or stale approval = no mutation. This remains a special-purpose amendment, not implicit new planning.
5. Execute an execution-lease-owned compare-and-swap transaction that checks CURRENT graph binding, original plan/hash, prior effective graph fingerprint, J1 source mutation journal, no J2 start, no checkpoint Tester attempt/output and unchanged non-amended contract. Record immutable before/after snapshot, approvals, timestamp and event; update both `boundary` and `definition_json` effective projection atomically. Change ancillary tables only if their data actually differs. Re-run `validate_against_plan` and the temporal validator **against the candidate effective graph before commit**.
6. Resume using exactly the effective graph contract in every reader. Keep `graph_version`, approved plan binding, existing J1 `code_workflow_state`, `code_checkpoints`, `code_reviews`, `code_cr_reviews`, verification, mutation journal, checklist and source bytes unchanged. Recompute the mature Local CR key/fingerprint before and after; reuse PASS only if exact key/target match; otherwise perform targeted independent revalidation without touching Coder source.
7. Make apply and resume **idempotent**: exact same amendment replay returns recorded result with no new event, crash before commit leaves original projection, crash after commit finds a single current revision and next action. On any guard failure: `GRAPH_REPAIR_BLOCKED`, no partial writes and precise Human-readable reason.

**Gate S2:** after a successful approved amendment, exact J1 source hashes + `change_set_id` + PASS records unchanged; original graph contract and approval history readable; amendment revision monotonic; crash/restart yields one effective contract, no duplicated repair or source writes.

## Phase S3 — Terminal handoff and Tester evidence enforcement

**GOAL:** resume the persisted J1 terminal outcome and execute the declared Tester checkpoint before enabling J2.

Implementation steps:
1. From the existing `REVIEW_PASS + TEST_PASS + CODE_CR_PASS` lineage, resume Local CR/terminalization idempotently. Reuse exact PASS only when its persisted evidence fingerprint matches; do **not** ask Coder to reimplement J1.
2. `complete_active_jobpack` commits J1 `DONE` separately from eligibility of J2. Resolve checkpoint `chk-j1-verify` at `AFTER_JOBPACK_SET` with prerequisite J1 `DONE`.
3. Tester runs its own real verification procedure, persists only `OBSERVED` result `ev-j1-verify` on the matching target; on FAIL/NEEDS_HUMAN/INTEGRATION_NOT_READY, stop with correct classification. **A failed Tester whose sole prerequisite is J1 `DONE` must not be pushed through the existing ACTIVE+`REVIEW_PASS` automatic PRODUCT_FAILURE repair path**, because that path cannot target a completed J1. Emit an explicit blocked/replan-or-reopen decision requiring separate Human-reviewed contract; do not modify J1 in this side plan. `VERIFICATION_RESULT=TEST_PASS` from Coder is **not a substitute** for Tester evidence.
4. `activate_eligible_jobpack_tx` may activate `jp-j2-idempotency` only after evidence is present, compatible, observed, not stale, and all dependencies hold. Ensure candidate choice is consistent with S1 evidence-precedence rules. Waiting for the scheduled Tester is **not** the same as a structural `PLAN_GAP`; a permanently impossible topology is a structural spec gap. Test that activation can't bypass the observed attempt and that an unrelated eligible Job Pack can still proceed.
5. Ensure restart between every transition takes the exact persisted next step, with at-most-once durable gate transitions; no duplicate Local CR, Tester outputs or JobPack DONE events.

**Gate S3:** `J1 DONE → Tester DUE → Tester PASS with OBSERVED output → J2 ACTIVE` in that order; failure branches fail closed; unchanged J1 source and change set.

## Phase S4 — Regression matrix, independent review and real Mac acceptance

**GOAL:** prove prevention and recovery in a real retained project, not only through green synthetic unit tests.

- Automated tests: S1 negative graph rejection and ordering under reversed lexical Job Pack IDs; S2 migration from exact **legacy graph v1** fixture (including WAL/read-back), valid/invalid amendment, pre-existing Tester attempt or prepared execution, J1 CR fingerprint before/after, duplicate application, wrong graph/plan/source, crash before/after atomic apply; S3 happy path, crash after J1 DONE, checkpoint retest, stale/mutated source, wrong evidence provenance or target, candidate scheduling and independence, Tester FAIL on J1 DONE requiring explicit blocked/manual contract, altered plan binding, lease mismatch, wrong workspace, J2 already started, conflicting repair, no duplicate event.
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
| J2 ID sorts lexicographically ahead of J1 evidence producer | do not activate J2 early; schedule producer/Tester deterministically |
| Missing output at a reachable Tester boundary | WAIT_FOR_TESTER; never report permanent graph cycle |
| Tester FAIL after J1 is DONE | no implicit J1 reopen or fake repair; explicit blocked/Human-reviewed route |
| Later independent Job Pack has unrelated evidence requirement | no premature blockage of preceding eligible pack |
| Already registered bad graph, no approval | no mutation; blocked with exact requested amendment |
| Approved one-field correction | original graph + amendment ledger preserved; same graph version and all J1 records unchanged |
| Existing graph v1 lacking contract hash/revision | deterministically enrolled from normalized tables; mismatch blocks amendment without mutation |
| Local CR key differs after amendment | old PASS preserved as history but not reused; targeted review, no Coder replay |
| Stale graph hash, changed source, wrong lease, wrong checkpoint or J2 already ACTIVE | fail closed, transaction rollback |
| Failed/replayed/crashed amendment | zero partial writes, idempotent exact replay only |
| J1 reviewed code and CR PASS but Tester output absent | do not activate J2; do not fabricate evidence |
| Tester result FAIL / NEEDS_HUMAN / stale | correct block/repair decision; no false TEST_PASS |
| Restart after J1 DONE or Tester OBSERVED | deterministic resume; no duplicate gate/event |
| Stock-QA external acceptance oracle | genuine independent final check, not replaced by prior three green smoke tests |

## Sequencing and authorization

`S0 → S1 → S2 → S3 → S4`. Do not parallelize S1 validator contract changes with a live S2 DB amendment. Keep one `main` source of truth; no stale experimental branch pileup. Code changes begin **only on explicit Human implementation approval**. Plan modification proposals require explicit approval when they expand scope. Work within GSA Local source; keep the Stock-QA fixture isolated. Existing runtime patches are subject to this validator and test suite, but are not deleted on speculation.

## Planner review findings and RL record (not an independent CR verdict)

**Source-grounded review findings addressed in this revision:**
- **P1 — runtime scheduler mismatch (MAJOR):** validation-only evidence edges do not constrain the Registry's lexicographic eligible-JobPack selection. Added S1 scheduler/validator alignment and reversed-ID acceptance case.
- **P2 — missing legacy baseline (MAJOR):** same-version amendment relies on contract revision/hash absent from the user's existing graph v1. Added deterministic legacy snapshot enrollment and migration-failure negative cases.
- **P3 — exact CR authority (MAJOR):** retained PASS rows are not sufficient if terminal boundary IDs or evidence fingerprint changes. Added explicit before/after key equivalence and targeted independent re-review on mismatch.
- **P4 — Tester FAIL on DONE prerequisite (MAJOR):** current auto product-repair path only supports the ACTIVE + REVIEW_PASS target. Added blocked/manual route without J1 rework.
- **P5 — dynamic readiness vs static topology (MODERATE):** missing evidence from a reachable Tester is not a graph cycle. Added separate WAIT_FOR_TESTER and GRAPH_ORDER_GAP outcomes plus negative regressions.

**RL checkpoints:**

- **RL1 — target/goal drift:** removed the tempting "register graph v2 and copy J1 PASS rows" shortcut because current schema binds records to graph version and ordinary registration retires active work. Added same-graph, append-only, fingerprinted amendment and a prohibition on silent rebinding.
- **RL2 — overbroad validator:** added event-level cycle checks instead of banning all same-milestone evidence dependencies. Kept positive cases for `AFTER_JOBPACK_SET`, `BEFORE_JOBPACK`, independent packs and cross-milestone gates.
- **RL3 — false green:** separated Coder `TEST_PASS` from Tester-owned `OBSERVED`, and CI regression from real Mac/oracle acceptance. Added fail-closed paths and crash/idempotence tests.
- **RL4 — approval/contract drift:** restricted registered graph correction to one checkpoint timing field; added independent reviewer and explicit Human approval, retained plan hash/goal/phase-goal invariants and deferred generic graph-version migration.
- **RL5 — scheduler and lifecycle consistency:** aligned the event DAG with actual runtime Job Pack eligibility, milestone COMPLETE behavior, checkpoint prerequisite states and the DONE-only Tester failure branch.
- **RL6 — existing-user migration:** prohibited assuming old v1 already has an effective graph hash; added deterministic read-back/reconstruction with fail-closed migration.
- **RL7 — review integrity / fail-closed:** require stable terminal CR key or targeted independent re-review, and keep temporary missing evidence distinct from irreversible ordering gaps.
- **Self-review result:** **PASS for Planner RL design readiness** after these findings were incorporated. **Independent CR recheck is required on the revised plan**; implementation, CI and real Mac acceptance are NOT claimed.

## Source references

- `src/execution_graph.rs`: `ExecutionGraph::validate`, `validate_checkpoints`, `validate_against_plan`, `CheckpointBoundaryKind`.
- `src/registry.rs`: `tester_boundary_state_tx`, `complete_active_jobpack`, `resolve_or_activate_work`, `register_execution_graph`, `approve_current_plan`, `current_graph_binding_tx`, `record_tester_attempt_evidence`.
- `src/workflow.rs`: `validate_job_builder_submission`, Job Builder PLAN_GAP and Reviewer/Local CR loops.
- `src/controller.rs`, `src/app.rs`: resume dispatcher, CR terminal handoff and Tester resolution.
- `tests/controller.rs`, `tests/job_builder.rs`, `tests/tester_evidence.rs`, `tests/tester_orchestration.rs`.
- `DETAIL_EXECUTION_PLAN.md`: rebaselined Tester checkpoint architecture; `WORKFLOW_LESSONS_LEARNED.md`: resume identity, no implicit restart, false-green evidence, fail-closed recovery.
- Real Mac SQLite observation 2026-10-14 and CI regressions documented in `.gsa/PROJECT_STATUS.md`.
