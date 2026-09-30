# GSA Workflow Lessons Learned

## Purpose

This document records **workflow-level lessons learned** from long-running GSA execution, interruption, recovery, review, and resume behavior.

It is intentionally **not a bug log**. Individual source bugs, test failures, and one-off implementation defects are not catalogued here unless they reveal a reusable workflow failure pattern.

The goal is to use these lessons to compare and harden both:

- **GSA Online** — ChatGPT-side control plane, durable Project Memory, Job/Run/Verification/Handoff lifecycle.
- **GSA Local** — local runtime, Registry, execution lease, checkpoint resolver, Coding/Reviewer/Test workflows.

The core question is:

> When a long workflow is interrupted, restarted, reviewed, repaired, or partially persisted, can GSA deterministically identify the exact current work state and continue from the correct next action without guessing from chat history or commit history?

---

# 1. Primary lesson: "resume" must be a first-class workflow operation

Current systems contain many useful state primitives, but they do not yet form one explicit resume contract.

GSA Online has:

- Project lifecycle;
- Job lifecycle;
- attributed Runs;
- parent/child Run relationships;
- Verification records;
- immutable Handoffs;
- exact target refs/revisions;
- serialized Job transitions.

GSA Local has:

- execution lease;
- execution graph;
- active Milestone / Job Pack;
- latest checkpoint;
- coding workflow state;
- review state;
- Tester attempt state;
- stale transition protection.

However, the system still relies too much on an agent deciding:

- whether to continue an existing Run;
- whether to create a child Run;
- whether an interrupted Run should be superseded;
- whether a workflow should restart from its beginning;
- which source revision is the authoritative resume point;
- which evidence remains valid after interruption.

That decision must move from model inference into a runtime-owned **Resume Protocol**.

---

# 2. Resume identity must be explicit

A recoverable execution needs more than:

```text
Job = IN_PROGRESS
Run = IN_PROGRESS
source head = X
```

The runtime must know the exact logical continuation identity.

Recommended durable structure:

```text
ResumeDescriptor
  project_id
  job_id
  task_id / jobpack_id
  workflow_kind
  workflow_stage
  active_run_id
  parent_run_id
  source_base_revision
  current_source_revision
  current_change_set_id
  latest_review_target
  latest_review_verdict
  latest_verification_refs
  pending_gate
  next_role
  replay_policy
  updated_at
```

This record should be produced by runtime state transitions, not reconstructed from prose.

A new chat/session should be able to resolve:

```text
resume()
  -> exact current workflow
  -> exact stage
  -> exact target
  -> exact next actor
  -> exact allowed action
```

without reading historical chat messages.

---

# 3. Starting a workflow and resuming a workflow are different operations

A major workflow hazard is treating a second invocation as a fresh start.

A fresh start is allowed to initialize state.

A resume must **not** reset valid in-progress state.

This distinction must exist explicitly:

```text
start_workflow(...)
resume_workflow(...)
recover_workflow(...)
```

They must not be aliases.

## Local implication

The local coding path currently initializes coding state through `begin_code_workflow()`.

That operation resets:

- checklist completion;
- TODO state;
- change-set binding;
- attempt counters;
- workflow status.

This behavior is correct for **new execution**, but unsafe if used implicitly after an interrupted process.

A restart must inspect persisted `code_workflow_state` first.

Expected behavior:

```text
no existing workflow
    -> START

existing CODER state
    -> RESUME CODER

existing REVIEWER state
    -> RESUME REVIEWER on exact change_set

existing INTERNAL_FIX state
    -> RESUME INTERNAL_FIX with exact findings

existing REVIEW_PASS
    -> continue to next declared gate

existing PAUSED
    -> explicit recovery decision
```

A resume must never call an initializer that destroys the state it is trying to recover.

---

# 4. Interruption is a normal state, not an exceptional state

Long sessions will end:

- model context ends;
- browser/app session ends;
- process crashes;
- network/tool provider fails;
- user pauses work;
- another chat continues the Job;
- external CI is still running;
- source changes exist but final Run persistence has not completed.

Therefore workflow records need a normal durable concept such as:

```text
INTERRUPTED
RECOVERABLE
RECOVERY_REQUIRED
```

rather than leaving an ordinary `IN_PROGRESS` Run with ambiguous meaning.

The runtime should know whether the previous actor:

- had not mutated source;
- mutated source but had not checkpointed;
- checkpointed but had not been reviewed;
- reviewed but had not persisted Verification;
- persisted Verification but had not completed Run;
- completed source work while the control plane failed to complete lifecycle metadata.

These states require different recovery behavior.

---

# 5. Source progress and control-plane progress must be reconciled separately

A common long-run condition is:

```text
source has advanced
control-plane lifecycle has not advanced
```

or the reverse.

The system must not assume that source HEAD and Run lifecycle move atomically.

A recovery resolver should compare:

```text
durable Run state
durable Verification state
durable Handoff state
source repository state
CI / deterministic verification state
```

and produce one reconciliation result:

```text
CONSISTENT
SOURCE_AHEAD
MEMORY_AHEAD
PARTIAL_PERSIST
STALE_TARGET
CONFLICT
```

Then recovery is deterministic.

Example policy:

```text
SOURCE_AHEAD + known mutation lineage
    -> attach exact new revision to recovery child Run

SOURCE_AHEAD + unknown lineage
    -> BLOCK / NEEDS_RECONCILIATION

MEMORY_AHEAD
    -> verify source target exists before continuing

PARTIAL_PERSIST
    -> complete missing persistence step idempotently
```

The agent should not manually rewrite state simply to satisfy a lifecycle validator.

---

# 6. Run target semantics must distinguish input target from output target

A Coder/Internal Fix Run naturally has two revisions:

```text
input_revision
output_revision
```

Using one ambiguous field named `target_revision` makes recovery and completion harder.

Recommended contract:

```text
Run
  input_target_ref
  input_target_revision

  output_target_ref?
  output_target_revision?
```

Reviewer Runs usually have:

```text
input target = reviewed exact source revision
output target = same revision
```

Coder Runs usually have:

```text
input revision = source before changes
output revision = source after changes
```

GOAL_RECHECK and deterministic Verification should bind to the **output revision**.

This should be enforced structurally instead of inferred by callers.

---

# 7. Verification persistence must be transactional from the caller's perspective

Online GSA currently separates:

1. build Verification record;
2. authorize memory mutation;
3. persist Verification through GitHub;
4. reference it from Run;
5. complete Run;
6. optionally create Handoff;
7. update terminal Run.

These are safe individual operations, but interruption between steps can leave partial durable state.

The control plane should expose an idempotent finalization primitive such as:

```text
finalize_run(
  run,
  output_target,
  verification_records,
  handoff
)
```

Internally it may still use GitHub writes, but it should return a reconciliation token and support retry.

Required property:

> Repeating finalization after an interrupted attempt must converge to one terminal state without duplicate Verification/Handoff records or manual SHA surgery.

---

# 8. Parent/child Run recovery needs explicit semantics

Internal Fix, Reviewer, Tester, and other corrective work often produce child Runs.

The runtime must define what happens to the parent.

Possible relationships should be explicit:

```text
CONTINUATION
CORRECTION
REVIEW
RETEST
RECOVERY
SUPERSEDING_RUN
```

A parent Run should not remain indefinitely `IN_PROGRESS` simply because work continued through child Runs.

Recommended rule:

- child starts as `RECOVERY` or `CORRECTION`;
- when child produces a terminal accepted result, runtime reconciles parent;
- parent becomes one of:
  - COMPLETED;
  - SUPERSEDED_BY_CHILD;
  - FAILED;
  - still active with explicit next stage.

This avoids "zombie" Runs that appear active after their real work moved elsewhere.

---

# 9. A fresh Reviewer gate must bind to exact output, not "latest source"

Long-running work often produces several commits after the originally reviewed revision.

The correct rule is:

```text
Reviewer PASS(target_revision = X)
```

not:

```text
Reviewer PASS(latest main)
```

If source changes after review, PASS does not automatically move forward.

The system should detect:

```text
review_target != current_output_target
    -> REVIEW_STALE
```

and require a fresh exact-target review.

This behavior should be shared by online and local GSA.

---

# 10. Evidence needs explicit validity and supersession semantics

Planning and execution history showed that old evidence can remain in durable records after a decision or revision has superseded it.

Manual "cleanup stale evidence" is a workflow smell.

Evidence should carry:

```text
produced_for
valid_from_revision
invalidated_by
superseded_by
applicability
status = CURRENT | SUPERSEDED | STALE
```

A new Plan revision, source revision, configuration change, or Human decision should invalidate incompatible evidence automatically.

Old evidence should usually be preserved for audit, not deleted from prose by hand.

The consumer should ask:

```text
give me CURRENT evidence applicable to target X
```

rather than relying on whichever evidence strings remain in a Job document.

---

# 11. Completion gates must explain exactly why they are blocking

A generic error such as:

```text
completion blocked: exact verification required
```

is insufficient for recovery.

A gate should return machine-readable diagnostics:

```json
{
  "gate": "CODER_COMPLETION",
  "status": "BLOCKED",
  "requirements": [
    {
      "id": "goal_recheck",
      "expected_run_id": "...",
      "expected_target_revision": "...",
      "expected_result": "PASS",
      "observed_refs": ["..."],
      "status": "MISMATCH"
    }
  ]
}
```

The agent should never need to guess whether the mismatch is:

- Run ID;
- producer;
- target ref;
- target revision;
- Verification result;
- persisted path;
- Run linkage;
- lifecycle;
- SHA freshness.

Good workflow errors are part of the control plane contract.

---

# 12. Checkpoint resume and Run resume are related but not identical

GSA Local already has a strong `latest_checkpoint` concept.

That checkpoint answers:

```text
Where is project execution?
```

A Run resume record answers:

```text
What exact actor operation was in progress at that checkpoint?
```

Both are required.

Recommended relationship:

```text
LatestCheckpoint
  -> Milestone
  -> Job Pack
  -> workflow stage
  -> active Run / attempt identity
```

The checkpoint resolver must not merely activate the correct Job Pack; it must restore the correct workflow stage and next actor.

---

# 13. Lease recovery must include execution identity, not only process identity

Local execution lease currently protects one project from multiple active owners.

That is necessary but not sufficient for crash recovery.

A new process taking over a stale lease should know which durable workflow attempt it inherits.

Recommended lease payload:

```text
lease_owner
lease_epoch
project
graph_version
jobpack
workflow_kind
run_or_attempt_id
stage
heartbeat
```

This prevents:

- duplicate workflow attempts;
- duplicate Tester attempts;
- starting Coder from zero after Reviewer had already begun;
- replaying non-idempotent work after crash.

---

# 14. Replay policy must apply to workflow actions, not only Tester commands

Tester already introduced useful replay concepts:

- OBSERVE_ONLY;
- IDEMPOTENT;
- NON_IDEMPOTENT.

The same principle should exist at workflow level.

Examples:

```text
read source                 IDEMPOTENT
create Verification record  IDEMPOTENT_BY_KEY
create Handoff              IDEMPOTENT_BY_KEY
start new Run               NON_IDEMPOTENT unless recovery key supplied
source mutation             CHANGESET_BOUND
Job transition              CAS / serialized
external deploy             NON_IDEMPOTENT / explicit recovery
```

Every resume path should know what can safely be replayed.

---

# 15. Online GSA needs a Work Resume API distinct from Project Resume

Current online `project_resume` resumes a PAUSED Project lifecycle.

That is not work recovery.

Online GSA needs an operation conceptually similar to:

```text
inspect_work_state(project_id, job_id?)
resume_work(run_id?)
recover_interrupted_run(run_id)
reconcile_job_state(job_id)
```

The API should return:

```text
authoritative_job
authoritative_task
active_or_interrupted_run
source target
latest accepted evidence
pending gate
next_role
allowed_next_operations
recovery_actions
```

Bare `op <project>` should use this resolver before selecting a Job.

This also prevents accidentally opening the most recent Job from another Project.

---

# 16. Local GSA needs a resume-aware workflow dispatcher

At startup local already resolves ActiveWork, but Coder invocation currently starts the coding workflow.

The dispatcher should instead do:

```text
state = inspect_workflow(active_work)

switch state:
  NONE
    -> start
  CODER
    -> resume coder
  REVIEWER
    -> resume exact reviewer target
  INTERNAL_FIX
    -> resume fix with persisted findings
  REVIEW_PASS
    -> continue next gate
  PAUSED
    -> recovery policy / user-visible blocker
  STALE
    -> reconcile
```

The same principle should later apply to:

- Tester checkpoint workflow;
- retest;
- Local CR;
- milestone verification.

---

# 17. "Latest commit" is not a resume protocol

Git history is useful recovery evidence, but it must not be execution state.

A correct resume should not require the agent to infer:

```text
this commit looks like the last valid internal fix
this later commit probably belongs to another interrupted attempt
this review probably targeted that one
```

Source history can confirm runtime state, but runtime state must identify which commits belong to the active change set.

Recommended durable source lineage:

```text
change_set_id
base_revision
mutation revisions[]
final_revision
produced_by_run
reviewed_by_run
verification_refs[]
```

---

# 18. Recovery must classify existing commits instead of deleting them by age

After an interrupted session there may be:

- obsolete work from a superseded architecture;
- valid work from the current architecture;
- incomplete but reusable work;
- commits from a different task;
- formatting/CI support commits.

Recovery should classify them:

```text
CURRENT_LINEAGE
REUSABLE_PARTIAL
SUPERSEDED
FOREIGN_TASK
UNKNOWN
```

Then policy decides whether to:

- retain;
- continue;
- revert;
- supersede;
- block for Human.

"Commit created before/after interruption" is not enough.

---

# 19. CI / deterministic verification availability is part of workflow readiness

A workflow should discover verification capability before reaching final completion.

The system should know early:

```text
verification command
execution surface
CI provider
OS requirement
network requirement
expected runtime
```

If deterministic verification cannot run, that should be a first-class capability state:

```text
AVAILABLE
NOT_CONFIGURED
ENVIRONMENT_BLOCKED
NOT_APPLICABLE
```

This prevents reaching the final Reviewer gate and only then discovering that required tests cannot be executed.

---

# 20. Proposed unified recovery state machine

Recommended shared conceptual model:

```text
NEW
  |
  v
RUNNING
  |
  +--> WAITING_EXTERNAL
  |
  +--> INTERRUPTED
           |
           v
      RECONCILING
        /   |    \
       /    |     \
 RESUMABLE BLOCKED SUPERSEDED
     |
     v
 RUNNING
     |
     +--> COMPLETED
     +--> FAILED
     +--> CANCELLED
```

For corrective loops:

```text
CODER
 -> REVIEWER
    -> PASS
    -> CHANGES_REQUIRED
         -> INTERNAL_FIX
         -> REVIEWER
```

After interruption, resume returns to the persisted node rather than beginning at CODER again.

---

# 21. Required recovery decision table

A runtime resume resolver should implement a deterministic table similar to:

| Durable state | Source state | Recovery |
|---|---|---|
| no active Run | no unbound source mutation | start new Run |
| Run IN_PROGRESS, no mutation | source unchanged | resume same stage |
| Coder checkpoint persisted | source matches change set | continue Reviewer |
| Reviewer CHANGES_REQUIRED persisted | source unchanged | continue Internal Fix |
| Internal Fix source mutation exists, checkpoint missing | known mutation lineage | recover checkpoint or create recovery child Run |
| Verification persisted, Run not terminal | target matches | finalize Run idempotently |
| Run terminal, Handoff exists | next task not started | follow Handoff |
| target revision changed after PASS | review stale | fresh Reviewer |
| source ahead with unknown lineage | unknown | BLOCK / reconcile |
| stale lease, old owner dead | durable attempt known | acquire lease and resume attempt |
| stale lease, durable attempt unknown | unknown | recovery required |

This table should exist in code/tests, not only documentation.

---

# 22. Negative tests that matter for resume correctness

Both online and local should eventually prove these cases:

1. Process/chat ends before any source mutation.
2. Ends after source mutation but before checkpoint.
3. Ends after checkpoint before Reviewer.
4. Ends during Reviewer.
5. Ends after CHANGES_REQUIRED before Internal Fix.
6. Ends during Internal Fix.
7. Ends after fresh source revision before GOAL_RECHECK.
8. Ends after Verification persistence before Run completion.
9. Ends after Handoff persistence before terminal Run update.
10. Resume is invoked twice concurrently.
11. Source changed externally while workflow was interrupted.
12. A stale Reviewer PASS exists for an older revision.
13. Parent Run remains open while recovery child Run completes.
14. A new session tries to restart coding on an existing REVIEWER state.
15. A stale lease exists but previous process is still alive.
16. A stale lease exists and previous process is dead.
17. A superseded Plan/graph still has active Run evidence.
18. Evidence applicable to an old target is offered to a new target.
19. Recovery is attempted from the wrong Project/Repo.
20. Replaying finalization creates no duplicate Verification/Handoff records.

---

# 23. Recommended invariant set

The following invariants should be shared across GSA Online and Local.

### Invariant A — one authoritative work cursor

At any time there is exactly one durable answer to:

```text
What work should continue next?
```

### Invariant B — no implicit restart

An existing recoverable workflow can never be reset by normal invocation.

### Invariant C — exact target binding

Review and verification always bind to an immutable exact target.

### Invariant D — recovery is idempotent

Repeating recovery/finalization converges to the same state.

### Invariant E — source and workflow lineage are linked

Every accepted source change set identifies the Run that produced it.

### Invariant F — stale evidence cannot silently satisfy a new target

Applicability is runtime-evaluated.

### Invariant G — ambiguous recovery fails closed

When lineage cannot be proven:

```text
BLOCKED / NEEDS_HUMAN
```

not guessed continuation.

### Invariant H — control-plane failure does not erase completed source work

Recovery can attach already-produced valid work without redoing it.

### Invariant I — no zombie active Run

If work has moved to a recovery/superseding child, the parent has an explicit terminal or delegated state.

### Invariant J — next task never starts from a partial prior gate

Task advancement requires durable accepted evidence for the preceding gate.

---

# 24. Practical design direction

The strongest design is to converge Online and Local around the same logical abstractions:

```text
WorkCursor
Run
ChangeSet
Checkpoint
Verification
Handoff
ResumeDescriptor
RecoveryDecision
Lease
EvidenceApplicability
```

Storage may remain different:

- Online: GitHub-backed durable Project Memory + control-plane API.
- Local: SQLite Registry + local source/runtime.

But the lifecycle semantics should match.

The same interrupted workflow should conceptually resolve the same way in both systems.

---

# 25. Immediate architectural conclusion

The observed failures do **not** imply that existing checkpoint, Run, lease, Verification, or Handoff concepts are wrong.

They show that these pieces need one additional layer:

# **Recovery / Resume Orchestrator**

Its job is not to execute product work.

Its job is to answer, deterministically:

```text
What was happening?
What durable work already exists?
What target is authoritative?
What evidence is still valid?
What operation was interrupted?
What may be replayed?
What must not be replayed?
Which Run owns the continuation?
Which actor goes next?
Which gate still blocks progression?
```

Only after this reconciliation should normal Planner/Coder/Reviewer/Tester orchestration continue.

---

# Final lesson

A robust agent workflow cannot treat restart as:

```text
reload context and try to continue
```

It must treat restart as:

```text
load durable state
+ reconcile source state
+ validate lineage
+ resolve exact work cursor
+ apply replay policy
+ resume one allowed transition
```

That should become a shared architectural contract for both **GSA Online** and **GSA Local**.
