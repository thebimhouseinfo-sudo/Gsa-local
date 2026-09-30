# Workflow Resume / Recovery — Lessons Learned

Status: architecture lessons learned only. This document does **not** start or implement T-ORCHESTRATION.

## Purpose

Capture workflow-level failures observed across long GSA sessions and commit history, without preserving verbose error logs.

The focus is not individual product bugs. The focus is failure modes that make work:

- impossible to resume,
- resumed from the wrong point,
- blocked by control-plane state,
- duplicated after restart,
- bound to stale source / plan / evidence,
- or incorrectly treated as complete.

This document compares the current **GSA online control plane** with **Gsa-local** and proposes hardening work.

---

## 1. Lessons learned from observed sessions

### LL-01 — Project / operation resolution must be deterministic

Observed pattern:

- an operation name can be resolved to the wrong durable Project when the agent infers from a recent Job instead of resolving the Project manifest / primary repo first.

Risk:

- correct Job state from another Project is loaded and treated as current work.

Lesson:

- `op <name>` must resolve Project first from exact handle/name/PRIMARY repo.
- never infer Project from latest Job or latest commit.
- ambiguity must block with an explicit candidate list.

Online status:

- guidance says not to guess ambiguous Projects, but resolution is still agent-mediated.

Local status:

- local is rooted in the current directory, so this exact ambiguity is smaller.

Required hardening:

- first-class Project resolver returning `project_id + primary_repo + lifecycle` before any Job lookup.

---

### LL-02 — Durable object identity must be atomic

Observed pattern:

- a Run file path and the `run_id` stored inside the file diverged and required a later repair commit.

Risk:

- parent/child lineage, verification refs, handoffs and completion gates can bind to different identities.

Lesson:

- a durable record must never be assembled by separately copying a generated path and generated JSON without validation.

Online status:

- `run_begin` returns a record/path and the caller persists it in a second external step.
- the Memory Repo can therefore contain identity mismatch if persistence is interrupted or stale data is copied.

Local status:

- SQLite writes are internal and transactional, so this class is much smaller.

Required hardening:

- online `run_begin` should persist atomically, or a `run_persist` validator must require:
  - filename UUID == body `run_id`,
  - handle derives from the same id,
  - parent exists,
  - Project/Job identity matches.

---

### LL-03 — Source can advance beyond durable Run lineage

Observed pattern:

- a long session ended with source commits already present while durable Run lineage had not captured all of them.
- a later session had to create a recovery Run and decide which already-present commits belonged to the interrupted task.

Risk:

- a new session may discard valid work, adopt stale work, or continue from an architecture version that has already been superseded.

Lesson:

- source HEAD and durable control-plane state are separate systems and must be reconciled before resuming.

Required recovery classification:

1. `EXACT_MATCH` — source == last durable source revision.
2. `SOURCE_AHEAD_KNOWN` — newer commits are already referenced by durable evidence.
3. `SOURCE_AHEAD_ORPHAN` — newer commits exist but have no durable Run ownership.
4. `MEMORY_AHEAD` — durable record claims output not present at source target.
5. `ARCHITECTURE_EPOCH_MISMATCH` — commits belong to a superseded plan/architecture.
6. `AMBIGUOUS` — Human decision required.

No new implementation Run should begin before this classification.

---

### LL-04 — A work Run needs a first-class resume primitive

Observed pattern:

- online recovery was performed by creating a new Run with a parent reference and manually reconstructing context.
- there is no online `run_resume` / `run_recover` primitive.
- `PROJECT resume` only resumes a PAUSED Project lifecycle, not interrupted work.

Risk:

- duplicate active Runs,
- wrong parent lineage,
- wrong input revision,
- handoff skipped or repeated.

Required online primitives:

- `run_status`
- `run_resume`
- `run_recover`
- `run_supersede`
- `run_completion_preflight`

`run_begin` should reject a second overlapping non-terminal Run unless the caller explicitly supplies a valid recovery/supersede relationship.

---

### LL-05 — Input revision and output revision must be different fields

Observed pattern:

- completion attempts repeatedly changed `target_revision` between the Run's starting revision and the revision produced by the fix.
- completion gating then became difficult to reconcile.

Root semantic problem:

- one field is carrying two meanings:
  - revision the Run started from,
  - revision the Run produced / verified.

Required model:

```text
input_revision
last_observed_revision
output_revision
reviewed_revision
verification_revision
```

Rules:

- `input_revision` is immutable after Run start.
- source-changing work may advance `output_revision`.
- Goal Recheck binds to `output_revision`.
- Reviewer binds to the exact `reviewed_revision`.
- any new source commit after Reviewer PASS invalidates that review coverage.

---

### LL-06 — Completion gates need machine-readable diagnostics

Observed pattern:

- a Coder completion gate stayed blocked while several combinations of target ref/revision and verification refs were tried.
- the agent had to infer hidden validator expectations from prior successful commits.

Risk:

- agents mutate durable metadata experimentally instead of following one canonical recovery path.

Lesson:

- a denied gate must not become a puzzle.

Required behavior:

`run_completion_preflight` returns typed reasons, for example:

```json
{
  "ready": false,
  "missing": ["GOAL_RECHECK"],
  "mismatch": {
    "expected_run_id": "...",
    "expected_output_revision": "...",
    "found_revision": "..."
  },
  "next_action": "record_goal_recheck"
}
```

The runtime should never require the agent to edit an IN_PROGRESS Run manually to guess how to satisfy completion.

---

### LL-07 — Goal criteria and verification availability are different concepts

Observed pattern:

- repository CI availability was temporarily represented as a goal criterion, changing the aggregate result even though the implementation criteria were otherwise met.

Lesson:

Separate:

1. **Acceptance criteria** — what the task must accomplish.
2. **Required verification checks** — commands/evidence required before completion.
3. **Environment limitations** — why a verification check cannot currently run.

Suggested typed states:

- criterion: `MET | PARTIAL | NOT_MET`
- verification: `PASS | FAIL | BLOCKED | NOT_APPLICABLE`

A required verification in `BLOCKED` must prevent final completion without turning the product criterion itself into `NOT_MET`.

---

### LL-08 — Required executable verification cannot be silently downgraded

Observed pattern:

- source review looked acceptable while repository CI was still unexecuted.
- once actual CI ran, it found formatting, compile and portability failures that static review had not caught.

Lesson:

- when a repository declares a real verification entrypoint such as `scripts/ci.sh`, final implementation completion must require observed execution unless the approved plan explicitly marks it optional.

Required fallback order:

1. local observed runner,
2. configured remote CI runner,
3. explicit `BLOCKED` / Human Retest.

Do not convert unavailable required verification into PASS.

Additional hard rule:

- verification infrastructure should be read-only with respect to product source.
- a verifier must not auto-format, auto-commit or push repairs.
- Coder performs fixes; verifier reruns checks.

---

### LL-09 — Stale planning evidence needs supersession semantics

Observed pattern:

- old product decisions remained embedded in planning evidence after Human decisions changed and later had to be manually cleaned.

Risk:

- resumed Planner/Reviewer work can consume both current and superseded facts as if both were valid.

Lesson:

Mutable planning facts cannot live only as free-form append-only prose.

Suggested evidence metadata:

```text
evidence_id
status = CURRENT | SUPERSEDED
valid_from_revision
invalidated_at_revision
superseded_by
source
```

Current plan context must include only CURRENT evidence by default.

---

### LL-10 — Exact graph-bound context is mandatory on resume

Observed pattern:

- Tester initially consumed the newest persisted plan rather than the plan revision bound to the current execution graph.

Lesson:

- "latest" is not the same as "current for this execution".

This has already been improved locally with exact graph-bound plan resolution.

General rule:

Every resumed role must derive context from immutable bindings:

```text
Project
 -> Job planning revision
 -> execution graph version
 -> task/jobpack/checkpoint
 -> exact source target/change set
 -> exact evidence applicability
```

Never reconstruct execution context from newest records alone.

---

## 2. Current GSA online vs Gsa-local

| Area | GSA online today | Gsa-local today | Gap |
|---|---|---|---|
| Project resolution | durable Project manifests, but caller still performs resolution steps | current filesystem root defines project | online needs enforced exact resolver |
| Durable transitions | GitHub Memory files + blob-SHA optimistic concurrency | SQLite transactions + event/checkpoint state | online multi-file transitions are not atomic |
| Run resume | no first-class work Run resume/recovery API | no general workflow resume coordinator | both incomplete |
| Coder restart | can manually create recovery Run | `CodingWorkflow.run()` always begins a new workflow | local can erase/reset resumable state |
| Planner restart | manual reconstruction | `PlanningWorkflow.run()` calls `begin_plan_workflow()` and resets route state | local cannot resume Planner/Reviewer/CR stage |
| Lease | no equivalent single local-process lease needed for web memory | PID lease exists | dead process can still block until stale timeout |
| Exact review target | strong exact-revision Reviewer rule | change-set bound Reviewer + stale journal checks | both good, but restart binding still needs recovery |
| Completion validation | strong gate, but diagnostics are not sufficiently explicit | registry transitions are explicit | online needs preflight diagnostics |
| Verification | records limitations honestly | deterministic Verification Controller | both need required-check gating policy |
| Evidence invalidation | revision-bound records, but prose facts can remain stale | graph/plan/evidence bindings stronger | both need explicit semantic supersession for mutable facts |

---

## 3. Direct local workflow gaps found in current code

### Local gap A — CodingWorkflow restart resets work

Current behavior:

`CodingWorkflow.run()` calls `begin_code_workflow()` every time.

`begin_code_workflow()`:

- clears checklist completion,
- resets todo status to PENDING,
- clears change-set id,
- resets Coder/Reviewer attempt counters,
- forces state back to `CODER`.

Existing tests explicitly verify that a new code workflow invalidates prior checklist completion.

This is valid for **START_NEW**, but unsafe for **RESUME_EXISTING**.

Required split:

```text
START_NEW
RESUME_EXISTING
RECOVER_INTERRUPTED
```

A restart must not implicitly choose START_NEW.

---

### Local gap B — PlanningWorkflow restart resets route

Current behavior:

`PlanningWorkflow.run()` always calls `begin_plan_workflow()`.

`begin_plan_workflow()` resets:

- current revision pointer,
- Reviewer attempts,
- CR attempts,
- route status to PLANNING.

Required:

- inspect persisted `plan_workflow_state` first,
- continue REVIEWER / LOCAL_CR / INTERNAL_FIX / PAUSED when valid,
- create a new plan only under explicit START_NEW or superseding-plan intent.

---

### Local gap C — App startup restores ActiveWork but not workflow stage

Current `App::new()`:

- acquires lease,
- resolves/activates ActiveWork.

It does not derive whether the interrupted workflow should resume at:

- Coder,
- Reviewer,
- Internal Fix,
- verification,
- Tester checkpoint,
- or Human/blocked state.

Required:

`ResumeCoordinator::resolve()` should run before any role dispatch.

---

### Local gap D — stale lease can block recovery too long

Current lease policy uses a long stale timeout and only checks whether the old PID is alive after the timeout is exceeded.

After a crash, a new process may therefore remain blocked until the lease ages out.

Required:

- heartbeat/renewed lease,
- process/session nonce,
- if the recorded local owner process is provably dead, allow immediate recovery,
- never rely only on elapsed timeout.

---

### Local gap E — no operator resume/status surface

Current local CLI only exposes:

- `/agent`
- `/model`
- `/config`

Required operator surface:

- `/status` — show exact durable workflow location.
- `/resume` — resume the exact safe ResumePoint.
- `/recover` — show divergence classification and recovery choices when automatic resume is unsafe.

---

## 4. Proposed shared ResumePoint contract

Both online and local should converge on a common semantic contract.

```json
{
  "project_id": "...",
  "job_id": "...",
  "task_ref": "...",
  "role": "coder",
  "architecture_epoch": "...",
  "plan_revision": 2,
  "graph_version": 7,
  "run_id": "...",
  "parent_run_id": "...",
  "stage": "REVIEWER",
  "input_revision": "...",
  "last_observed_revision": "...",
  "output_revision": "...",
  "reviewed_revision": "...",
  "pending_gate": "REVIEWER_PASS",
  "next_action": "resume_reviewer",
  "recovery_class": "EXACT_MATCH"
}
```

The record should contain **state, not model reasoning**.

---

## 5. Resume preflight algorithm

Every new chat / process restart that continues existing work should perform:

1. resolve exact Project;
2. resolve active Job and approved planning revision;
3. find non-terminal Runs and last terminal handoff;
4. read current source HEAD and working-tree state;
5. compare source against durable input/output/review revisions;
6. validate architecture/plan epoch;
7. classify divergence;
8. select one safe ResumePoint;
9. only then dispatch a role.

Fail closed on ambiguity.

---

## 6. Atomicity lessons

### Online

Current Memory persistence spans multiple GitHub file writes:

- Verification,
- Handoff,
- terminal Run.

A crash can occur between writes.

Preferred direction:

- server-side atomic persistence, or
- append-only transition/event record from which Run/Handoff projections are rebuilt.

At minimum, recovery must recognize:

- orphan Verification,
- orphan Handoff,
- IN_PROGRESS Run with durable output revision,
- terminal Run missing handoff,
- source commit with no Run ownership.

### Local

SQLite already gives stronger transaction boundaries.

Do not lose this advantage by resetting durable workflow state on every user dispatch.

---

## 7. Hardening tests to add

### Shared workflow tests

- restart after source mutation but before Goal Recheck;
- restart after Goal Recheck but before Run completion;
- restart after Handoff persistence but before terminal Run update;
- source ahead of durable Run by one or more task-owned commits;
- source ahead with unrelated user-owned commits;
- architecture revision changes while old Run is IN_PROGRESS;
- second Run begin while overlapping non-terminal Run exists;
- review PASS followed by a new source commit;
- stale/superseded planning evidence not injected after resume;
- required CI unavailable => BLOCKED, never PASS.

### Local-specific tests

- restart while code state = CODER;
- restart while code state = REVIEWER;
- restart while code state = INTERNAL_FIX;
- restart while code state = PAUSED;
- `begin_code_workflow` cannot overwrite resumable state without explicit START_NEW;
- restart PlanningWorkflow at REVIEWER / LOCAL_CR / INTERNAL_FIX;
- dead lease owner recovers without long timeout;
- live lease owner cannot be displaced;
- startup returns exactly one ResumePoint.

### Online-specific tests

- Run file path/body id mismatch is rejected;
- duplicate non-terminal Run for same task is rejected or requires explicit recovery;
- completion preflight explains exact missing/mismatched evidence;
- output revision cannot overwrite immutable input revision;
- orphan Handoff/Verification is reconciled idempotently;
- Project op cannot resolve via a Job belonging to another Project.

---

## 8. Proposed scope correction for future T-RESUME

Current planning treats much of resume work as Tester/checkpoint recovery.

Lessons learned show the scope should be broader.

T-RESUME should cover at least:

1. work-session / Run recovery,
2. Planner workflow recovery,
3. Coder ↔ Reviewer ↔ Internal Fix recovery,
4. Tester checkpoint/attempt recovery,
5. source-vs-memory reconciliation,
6. lease recovery,
7. stale plan/evidence invalidation,
8. idempotent Handoff/Verification completion,
9. operator status/resume/recover surfaces.

Tester checkpoint resume remains one part of the larger contract, not the whole contract.

---

## 9. Priority

### P0 — before trusting long-session autonomous work

- explicit ResumePoint / resume preflight;
- source-vs-memory reconciliation;
- no reset-on-restart in local Planner/Coder workflows;
- online non-terminal Run recovery;
- immutable input revision vs output revision;
- completion preflight diagnostics.

### P1

- stale lease recovery;
- atomic online Run/Verification/Handoff persistence;
- typed planning-evidence supersession;
- required verification gating + remote runner capability.

### P2

- operator UX (`status/resume/recover`);
- broader restart chaos tests;
- automated orphan-record reconciliation.

---

## 10. Core principle

A new session must never answer:

> "What should I continue?"

by guessing from chat history, newest commit, newest Job, or newest plan.

It must answer from a deterministic reconciliation of:

```text
durable workflow state
+ exact source state
+ exact approved plan/graph
+ exact verification/review coverage
= one safe ResumePoint
```

If that equation has more than one valid answer, the workflow is not resumable automatically and must return an explicit recovery decision instead of continuing.
