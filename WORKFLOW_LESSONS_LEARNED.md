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
  plan_revision
  plan_hash
  execution_graph_version
  task_id / jobpack_id
  workflow_kind
  workflow_stage
  active_run_id
  parent_run_id
  input_source_target_set
  output_source_target_set?
  current_change_set_id
  latest_review_target
  latest_review_verdict
  latest_verification_refs
  pending_gate
  next_role
  continuation_policy
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
  input_source_target_set
  output_source_target_set?
```

Each SourceTargetSet contains one exact repository target in SINGLE-repo work and the exact required repository/revision combination in MULTI-repo work.

Reviewer Runs usually have:

```text
input SourceTargetSet = reviewed exact target set
output SourceTargetSet = same target set
```

Coder Runs usually have:

```text
input SourceTargetSet = source before changes
output SourceTargetSet = exact source set after changes
```

GOAL_RECHECK and deterministic Verification should bind to the **output SourceTargetSet**.

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


---

# 26. Handoff is not authorization to activate the next Task

A Handoff is durable routing information. It records the expected next role, remaining work and required inputs.

It must not itself mean:

    next Task has started
    next phase is active
    execution may cross a Human stop boundary

Observed workflow behavior around T-EXECUTION showed why this distinction matters: a valid Handoff may exist while the Human explicitly requires stopping before T-ORCHESTRATION.

Recommended contract:

    Handoff
      activation_policy = EXPLICIT_START | AUTO_CONTINUE

Default at Task/phase boundaries:

    EXPLICIT_START

Only an approved plan may opt into AUTO_CONTINUE.

This prevents a successful prior gate from silently advancing execution beyond the Human-authorized boundary.

---

# 27. Required verification and verifier mutation authority are separate contracts

A required deterministic check may be unavailable even when source review looks good.

The runtime should represent:

    goal_status
      MET | PARTIAL | NOT_MET

    verification_status
      PASS | FAIL | BLOCKED | NOT_APPLICABLE

    completion_gate
      READY | NOT_READY | NEEDS_HUMAN

A required verification in BLOCKED or UNOBSERVED state must prevent terminal success without rewriting the product criterion itself as failed.

The verification surface must also remain read-only with respect to product source by default.

A verifier/CI workflow must not auto-format, auto-commit or push source repairs merely to make its own check pass. Correct routing is:

    verifier finds failure
      -> Coder fixes
      -> verifier reruns

This preserves role boundaries and keeps evidence trustworthy.

---

# 28. Gate evidence needs a canonical current pointer

Append-only Verification history is valuable, but completion gates should not require the model to manually replace arrays of refs until the expected record happens to be selected.

Recommended runtime-owned pointers:

    latest_goal_recheck
    latest_code_review
    latest_tester_verification
    latest_required_ci

When a newer exact-target record supersedes an older record, update the canonical pointer transactionally while keeping all historical records immutable.

Completion validators consume the canonical semantic state.

This removes a class of recovery work where the implementation is valid but the Run remains blocked by bookkeeping reference wiring.

---

# 29. Project resolution precedes Job resolution

Opening an operation must first resolve the exact Project identity and registered PRIMARY/source repository before selecting any Job.

Never infer the Project from:
- the most recent Job;
- the most recent Run;
- recent conversation context alone.

If a name can resolve to multiple Projects, return explicit candidates and block execution.

This is especially important for Online GSA, where durable Project Memory for many repositories shares one Memory Repo.



---

# 30. Online GSA needs an authoritative active-Run / WorkCursor index

The Online control plane can create and complete Runs, but there is no first-class Run-list/open-Run resolver in the current tool surface.

That means a resumed session may need to discover execution state by scanning durable files or reconstructing lineage from Git history.

That is not sufficient for deterministic resume.

The control plane should maintain one runtime-owned **WorkCursor** per active Job / execution scope.

Recommended structure:

```text
WorkCursor
  project_id
  job_id
  plan_revision
  plan_hash
  execution_graph_version
  active_task_ref? / jobpack_id?
  active_run_id?
  active_run_role?
  workflow_stage
  pending_gate
  input_source_target_set
  output_source_target_set?
  latest_handoff_ref?
  continuation_policy
  cursor_version
```

Properties:

- exactly one authoritative cursor for one active Job execution lineage;
- updated through compare-and-swap / serialized transition;
- points to the Run that currently owns continuation;
- terminal child Runs may exist without becoming the continuation owner unless the cursor is explicitly advanced;
- historical Runs remain immutable audit records;
- resume reads the cursor first rather than searching for the newest Run.

If several IN_PROGRESS Runs exist because of partial persistence or older behavior, recovery must reconcile them before assigning the cursor.

This provides the durable answer to:

```text
Which Run owns the next action?
```

---

# 31. Planning resume requires the same protection as Coding resume

The current Local planning entry path calls `begin_plan_workflow()` unconditionally.

That operation resets:

- current planning revision pointer;
- Reviewer attempt count;
- Local CR attempt count;
- workflow status back to PLANNING.

Therefore Planning has the same restart hazard already identified for Coding.

Local needs a resume-aware planning entry resolver:

```text
resolve_planning_entry()
```

Suggested decisions:

```text
no planning state
  -> START_PLANNING

PLANNING with no persisted revision
  -> RESUME_PLANNER

REVIEWER
  -> RESUME_REVIEWER on exact persisted revision/hash

PLANNER after CHANGES_REQUIRED
  -> RESUME_PLAN_FIX with exact findings

LOCAL_CR
  -> RESUME_LOCAL_CR on exact reviewed revision/hash

INTERNAL_FIX
  -> RESUME_PLAN_INTERNAL_FIX

JOB_BUILDER
  -> resume Job Builder / graph registration boundary

PAUSED
  -> explicit recovery policy

APPROVED / graph registered
  -> do not restart planning
```

The same rule applies as Coding:

> An initializer may only initialize absent/new state. It may not be used as the normal entrypoint for a recoverable workflow.

Planning revision counters and review budgets must survive process restart.

---

# 32. Resume packets must be reconstructible without chat/model history

Local currently keeps session/model message history in process memory, and individual workflow loops build transient message packets.

After process restart those message buffers are gone.

That is acceptable only if the next safe workflow action can be reconstructed entirely from durable structured state.

Do **not** make chat transcript persistence a requirement.

Instead persist the minimal resume inputs needed to deterministically rebuild the next agent packet:

```text
workflow stage
exact plan/job/task/jobpack
input target
current output/change_set
latest findings
latest accepted checkpoint
attempt counters
required evidence refs
pending gate
continuation policy
```

Then:

```text
durable structured state
  + current source truth
  + current harness
  -> regenerated agent packet
```

Conversation history may improve explanation, but must never be required for correctness.

This also prevents hidden reasoning or stale conversational assumptions from becoming an execution dependency.

---

# 33. Multi-file durable transitions need transaction identity, not only ordered writes

Online GSA deliberately persists durable state through multiple GitHub writes:

- Run;
- Verification;
- Handoff;
- terminal Run replacement;
- Job lifecycle updates.

Ordering these writes carefully is necessary, but not sufficient for crash recovery.

Every logical durable transition should have a stable **transition_id / idempotency_key**.

Recommended pattern:

```text
TransitionIntent
  transition_id
  operation_kind
  project_id
  job_id
  run_id?
  expected_precondition
  intended_objects[]
  status = PREPARED | COMMITTED | ABORTED
```

Recovery behavior:

```text
no intent
  -> transition never started

PREPARED + some objects exist
  -> reconcile and finish idempotently

PREPARED + conflicting objects
  -> BLOCKED_RECONCILIATION_REQUIRED

COMMITTED
  -> replay returns existing result, creates nothing new
```

The idempotency key should be accepted by operations that create:
- Run;
- Verification;
- Handoff;
- external verification request.

This generalizes the earlier `finalize_run()` lesson from one completion path into a full crash-consistency contract.

---

# 34. Local lease ownership needs fencing, not PID identity alone

The current Local lease owner is based on:

```text
pid:<process_id>
```

and stale-owner liveness is checked using the PID.

PID alone is not a durable process identity:

- operating systems can reuse PIDs;
- a restarted process may receive the same PID as a dead prior owner;
- an unrelated process can make an old PID appear alive;
- ownership takeover needs protection against stale writers, not only stale detection.

Recommended lease contract:

```text
Lease
  owner_nonce
  process_id
  process_start_identity
  lease_epoch / fencing_token
  acquired_at
  heartbeat_at
  work_cursor
```

On takeover:

1. runtime proves the previous lease is recoverable/stale;
2. increments a monotonic fencing token;
3. new owner receives that token;
4. every mutating Registry transition requires the current fencing token.

An old process with a stale token must be rejected even if it continues running.

The lease should therefore protect both:

```text
single active process
+
single authoritative mutation epoch
```

This is necessary for safe resume after crashes, PID reuse, long suspension, or competing terminals.

---

# 35. External operations need durable continuation identity

Some workflow steps outlive the chat/process that started them:

- CI runs;
- deployment;
- browser/runtime test jobs;
- remote builds;
- approvals;
- other provider-side operations.

A restart must not blindly start them again.

Persist an `ExternalOperation` record:

```text
ExternalOperation
  operation_id
  provider
  provider_operation_id
  operation_kind
  target_revision
  requested_by_run
  replay_policy
  status
  started_at
  last_observed_at
  result_ref?
```

Resume behavior:

```text
RUNNING
  -> inspect the same provider_operation_id

SUCCEEDED
  -> consume existing result

FAILED
  -> route failure

UNKNOWN
  -> reconcile before retry

NON_IDEMPOTENT + uncertain
  -> NEEDS_HUMAN / explicit recovery
```

Example:

If GitHub Actions run `#78` is already executing for revision X, a resumed session should observe run `#78` rather than dispatching another identical CI run unless retry policy explicitly allows it.

External operation identity should be included in the WorkCursor / pending gate state.

---

# 36. Retry and attempt budgets are monotonic across resume

Bounded loops only protect the system if restart cannot reset their counters.

The following counters must be durable and monotonic for the same logical workflow lineage:

- Planner revision/review attempts;
- Reviewer attempts;
- Local CR attempts;
- Coder/Internal Fix attempts;
- Tester tool/report correction rounds where recovery spans process boundaries;
- retest attempts;
- recovery attempts when bounded.

Resume must restore the current budget state.

It must never do:

```text
restart process
  -> attempt counter = 0
  -> retry same failing loop indefinitely
```

A new budget may begin only when runtime creates a genuinely new logical workflow lineage, such as:

- approved new Plan revision;
- explicitly superseding Run;
- new Tester target/attempt;
- Human-authorized retry class.

The transition that resets a budget must be explicit and auditable.

---

# 37. Human stop / continuation intent must be durable at the WorkCursor level

Section 26 establishes that Handoff is not authorization to activate the next Task.

The review adds one stronger requirement:

The Human continuation policy must not live only inside an optional Handoff, because a resumed session may resolve work before reading or producing that Handoff.

Persist the boundary on the authoritative WorkCursor / execution boundary:

```text
continuation_policy
  EXPLICIT_START
  AUTO_CONTINUE

stop_after_task?
stop_after_gate?
human_gate_ref?
```

When the policy is `EXPLICIT_START`:

```text
prior Task PASS
  -> cursor = WAITING_EXPLICIT_START
  -> no next Task activation
```

A later "continue" becomes an explicit state transition, not an interpretation of old conversational intent.

This prevents a new session from crossing a Human-requested stop boundary simply because the previous Task already produced a valid Handoff.



---

# 38. Source mutation needs a durable write-ahead identity

Local source mutation currently has a crash window:

```text
write file
  -> update in-memory mutation journal
  -> later persist code checkpoint
```

If the process dies:

- after the file write but before the journal append;
- after the journal append but before checkpoint persistence;
- or during a non-atomic file replacement;

then source may be ahead of durable workflow lineage with no authoritative change-set record.

That turns resume into forensic reconstruction.

The mutation layer therefore needs a write-ahead contract.

Recommended structure:

```text
MutationIntent
  mutation_id
  run_or_attempt_id
  path
  expected_before_hash
  intended_after_hash
  operation = CREATE | REPLACE
  status = PREPARED | APPLIED | CHECKPOINTED | ABORTED
```

Safe write sequence:

```text
1. persist PREPARED MutationIntent
2. verify expected before-state
3. write to temporary sibling file
4. fsync when required
5. atomic rename/replace
6. verify resulting hash
7. mark mutation APPLIED
8. include mutation_id in ChangeSet/checkpoint
9. mark CHECKPOINTED
```

Recovery rules:

```text
PREPARED + source still before_hash
  -> safe to retry or abort

PREPARED + source == intended_after_hash
  -> adopt as APPLIED, do not rewrite

PREPARED + source is neither hash
  -> BLOCKED_SOURCE_DIVERGENCE

APPLIED but not CHECKPOINTED
  -> reconstruct the same ChangeSet from durable mutation intents

CHECKPOINTED
  -> ordinary resume
```

The write-ahead mutation identity should also prevent a later restart from accidentally treating a legitimate interrupted edit as unknown/user-owned source drift.

This contract complements Git/source history; it does not require every local mutation to be committed to Git immediately.

---

# 39. Durable state schema evolution is part of resume correctness

Resume may happen after the GSA runtime itself has been upgraded.

Therefore old durable state can be read by newer code.

This includes:

- Local SQLite Registry schema;
- checkpoint/event payloads;
- execution graph definitions;
- Run/Verification/Handoff JSON contracts;
- WorkCursor / ResumeDescriptor records;
- evidence applicability schemas.

Schema evolution must not silently reset or reinterpret active work.

Recommended contract:

```text
StateSchema
  schema_version
  minimum_readable_version
  migration_id
  migration_status
```

Startup behavior:

```text
current version
  -> normal resume

older compatible version
  -> transactional migration
  -> validate invariants
  -> resume

older version requiring semantic rebaseline
  -> migrate durable records
  -> explicitly mark affected graph/evidence/work cursor STALE or SUPERSEDED
  -> route to deterministic recovery point

unknown/newer incompatible version
  -> BLOCKED_STATE_VERSION
```

Migration requirements:

- migrations are ordered and idempotent;
- migration success is persisted before ordinary workflow resumes;
- active Run/Task/JobPack identities are preserved when semantics remain compatible;
- semantic incompatibility is explicit, never translated into a fresh workflow silently;
- attempt budgets and review/evidence lineage survive compatible migrations;
- invalidated evidence is retained for audit but cannot satisfy current gates;
- a migration may move the WorkCursor backward only through an explicit recovery/rebaseline transition with recorded reason.

Online durable JSON should follow the same principle even if storage is Git files rather than SQLite.

Every durable record type should expose a schema/version contract so a future control-plane version can distinguish:

```text
legacy but valid
migratable
semantically stale
unsupported
```

Runtime upgrade is therefore another resume event, not a reason to discard workflow state.


---

# 40. Resume identity must bind execution graph and multi-repository SourceTargetSet

A WorkCursor cannot safely identify work using only:

```text
job_id
task_id / jobpack_id
single source revision
```

because:

- the same logical task/jobpack identifier may exist across superseding Plan/ExecutionGraph revisions;
- GSA Online supports MULTI-repository Projects;
- one repository may advance while another remains unchanged or partially updated;
- cross-repository evidence may only be valid for one exact combination of repository revisions.

Therefore every resumable execution identity must include the planning/graph binding:

```text
plan_revision
plan_hash
execution_graph_version
task_id / jobpack_id
```

The authoritative source target should be modeled as a set:

```text
SourceTargetSet
  repository_targets[]
    repository_url / registry_id
    role
    relation
    ref
    revision
    mutation_scope
    ownership
```

For SINGLE-repo work this set contains one element.

For MULTI-repo work it records the exact repository combination that produced, reviewed or verified the result.

Resume reconciliation must be per repository and aggregate:

```text
repo A = CONSISTENT
repo B = SOURCE_AHEAD
repo C = UNCHANGED
  -> aggregate = PARTIAL_SOURCE_ADVANCE
```

The runtime must not collapse this into one "latest revision".

Recommended recovery outcomes include:

```text
SOURCE_SET_CONSISTENT
PARTIAL_SOURCE_ADVANCE
PARTIAL_PERSIST
FOREIGN_REPO_MUTATION
STALE_GRAPH_BINDING
SOURCE_SET_CONFLICT
```

Rules:

- a Run created for graph version X cannot resume against graph version Y merely because task/jobpack names match;
- a Reviewer PASS binds to the exact SourceTargetSet it reviewed;
- Verification/evidence applicability includes all repository dimensions that materially affect the result;
- a cross-repo Job may advance only when required repository targets satisfy the approved topology;
- source changes in a registered but out-of-scope repository must not be silently adopted into the active change set;
- partial cross-repo mutation after interruption must be reconciled explicitly rather than rolled forward by assumption.

Cross-repository writes do not need to pretend to be globally atomic.

Instead the durable transition should record each repository result and expose partial completion so recovery can finish, compensate, supersede or block deterministically.

The WorkCursor defined earlier should therefore include at minimum:

```text
project_id
job_id
plan_revision
plan_hash
execution_graph_version
active_task_ref / jobpack_id
active_run_id
workflow_stage
input_source_target_set
output_source_target_set?
pending_gate
continuation_policy
cursor_version
```

This prevents resume from crossing either:

- a repository boundary;
- a superseded plan/graph boundary;
- or an exact-target review/evidence boundary.
