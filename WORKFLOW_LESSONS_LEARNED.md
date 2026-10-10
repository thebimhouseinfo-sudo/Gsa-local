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
Reviewer PASS(output_source_target_set = X)
```

not:

```text
Reviewer PASS(latest source state)
```

If source changes after review, PASS does not automatically move forward.

The system should detect:

```text
reviewed_output_source_target_set != current_output_source_target_set
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

---

# 41. Human-directed architecture interruption requires Rebaseline + Salvage, not restart

Not every interruption is a crash or tool failure. The Human may intentionally stop active work because architecture, workflow, product contracts, ownership, repository topology, or role boundaries have changed materially.

This event is a first-class `HUMAN_REBASELINE_REQUIRED` transition. It must be neither ordinary resume nor blanket restart-from-zero.

## 41.1 Freeze before reinterpreting old work

When the Human changes architecture during active work:

1. stop new source mutation for the affected scope;
2. freeze the current WorkCursor;
3. snapshot the active plan revision/hash, graph version, Task/JobPack, open Runs, SourceTargetSet, ChangeSets/MutationIntents, review/verifications, and external operations;
4. record the Human directive as a durable requirement boundary.

Recommended record:

```text
HumanRebaselineDirective
  rebaseline_id
  project_id
  previous_plan_revision
  previous_plan_hash
  previous_graph_version
  frozen_source_target_set
  affected_scope
  directive_summary
  effective_at
  continuation_policy = REBASELINE_REQUIRED
```

No old Run in affected scope may continue normal mutation after this point.

## 41.2 Architecture epoch prevents old Runs from resuming into the new contract

Introduce a monotonic logical `architecture_epoch`, or equivalently bind affected work to `rebaseline_id`.

```text
epoch N
  -> FROZEN_BY_REBASELINE

epoch N+1
  -> created only after revised plan/graph approval
```

A Run from epoch N cannot resume against epoch N+1 merely because Job/Task ids match or because its source changes still exist. Reuse requires an explicit salvage/adoption decision.

## 41.3 Preserve source first; classify before revert

The default action is `PRESERVE + CLASSIFY`, not `RESET / REVERT ALL`.

Classify existing work at the smallest practical semantic unit: ChangeSet, commit, file group, or bounded feature slice.

```text
ADOPT_AS_IS
ADOPT_REVIEW_REQUIRED
ADAPT_TO_NEW_CONTRACT
PARTIAL_SALVAGE
SUPERSEDED_NO_LONGER_USED
REVERT_REQUIRED
FOREIGN_TASK
USER_OWNED_UNRELATED
UNKNOWN_NEEDS_HUMAN
```

- `ADOPT_AS_IS`: behavior and contract remain valid under the new architecture.
- `ADOPT_REVIEW_REQUIRED`: likely compatible, but exact-target review/reverification is required.
- `ADAPT_TO_NEW_CONTRACT`: implementation remains useful but interface/ownership/workflow assumptions changed.
- `PARTIAL_SALVAGE`: only part of the old work is reusable.
- `SUPERSEDED_NO_LONGER_USED`: code can remain temporarily but is no longer authoritative.
- `REVERT_REQUIRED`: leaving the change conflicts with the new approved architecture.
- `FOREIGN_TASK`: belongs to another planned task.
- `USER_OWNED_UNRELATED`: preserve; GSA has no authority to remove it.
- `UNKNOWN_NEEDS_HUMAN`: fail closed.

A blanket revert is prohibited unless the Human explicitly requests it or the rebaseline proves the whole change set incompatible.

## 41.4 Salvage decisions require dependency-impact review

Compatibility is not determined only by whether old code still compiles. For each old ChangeSet or completed Task, evaluate new architecture contracts, ownership/role changes, callers/consumers, schema changes, evidence applicability, verification assumptions, downstream dependencies, and repository topology.

Recommended impact result:

```text
UNCHANGED
COMPATIBLE_WITH_REVIEW
REQUIRES_ADAPTATION
INVALIDATES_DOWNSTREAM
SUPERSEDED
CONFLICTS
```

If an upstream contract changed, downstream work depending on that contract may become stale even if its source was not directly edited. Rebaseline must propagate dependency impact rather than reviewing commits in isolation.

## 41.5 New plan starts from the live salvaged baseline

Planner must not plan from the pre-interruption repository snapshot. The new baseline is:

```text
frozen live SourceTargetSet
+ accepted salvage decisions
+ Human architecture directive
```

The rebaseline plan should explicitly mark prior work as reused unchanged, reused after review, adapted, superseded, reverted, or reimplemented. This prevents duplicate work and preserves already-valid implementation.

## 41.6 Old Runs need semantic terminal states

Affected Runs should not remain ambiguous `IN_PROGRESS`.

```text
SUSPENDED_BY_REBASELINE
SUPERSEDED_BY_REBASELINE
ADOPTED_INTO_REBASELINE
REQUIRES_ADAPTATION
```

Do not mark valid-but-superseded work as ordinary FAILED. Do not mark old Runs COMPLETED under the new architecture unless explicitly revalidated/adopted. The WorkCursor moves to `REBASELINING` until the new plan/graph is approved.

## 41.7 Evidence is revalidated by applicability, not discarded wholesale

Prior evidence may still be useful:

```text
same target + same relevant contract
  -> may remain CURRENT

source unchanged but architecture interpretation changed
  -> REVALIDATION_REQUIRED

contract/environment/applicability dimension changed
  -> STALE / SUPERSEDED

measured runtime fact remains invariant and applicability still matches
  -> ADOPTABLE
```

This is especially important for Tester-produced empirical evidence. The default is re-evaluate applicability, not delete all old evidence.

## 41.8 Rebaseline must be reviewed before execution resumes

Required flow:

```text
Human architecture directive
  -> FREEZE affected work
  -> SALVAGE / IMPACT analysis
  -> Planner revised plan
  -> Reviewer
  -> Local CR when architecture/governance scope requires it
  -> new approved plan revision
  -> new execution graph
  -> bind adopted source/evidence
  -> resume execution
```

Coder must not continue adapting affected code while the new architecture plan is unresolved, except for explicitly authorized stabilization work.

## 41.9 Prior commits map into the new graph explicitly

The new graph should record provenance:

```text
Task / JobPack
  adopted_from_change_sets[]
  adopted_from_runs[]
  supersedes_task_refs[]
  required_reverification[]
```

This prevents migrated work from looking newly generated with no lineage and lets Job Builder avoid recreating already-accepted implementation work.

## 41.10 Rebaseline completion criteria

A Human-driven architecture interruption leaves `REBASELINING` only when:

1. affected old work has been classified;
2. unknown ownership/scope items are resolved or explicitly blocked;
3. source baseline is captured;
4. downstream impact is mapped;
5. stale evidence/gates are invalidated;
6. reusable evidence is rebound with explicit applicability;
7. revised plan is approved;
8. new execution graph is registered;
9. WorkCursor is bound to the new epoch/graph;
10. no old-epoch Run can mutate affected scope.

Only then may normal resume/orchestration continue.

## 41.11 Key invariant

Human architecture changes must preserve useful work without preserving obsolete authority:

```text
preserve source value
+ invalidate old authority
+ classify compatibility
+ explicitly adopt what survives
+ replan what changed
```

This is the safe middle ground between blindly resuming old work and throwing away all previous implementation.

---

# 42. Integration should happen at bounded reviewed checkpoints, not after unbounded commit accumulation

Long-running implementation should not wait until a branch or work lineage contains hundreds of commits before integration.

Large unintegrated histories make resume, architecture rebaseline, source reconciliation, review, conflict handling, salvage classification, and rollback much harder.

GSA should therefore define bounded integration checkpoints.

## 42.1 Preferred integration boundary

A good integration point is usually reached when:

1. the current Task / JobPack / bounded feature slice is complete;
2. Coder self-check has passed;
3. required Reviewer gate has passed;
4. required Tester / deterministic verification for that boundary has passed or is explicitly not applicable;
5. no unresolved blocking finding remains;
6. the exact SourceTargetSet is known;
7. the next work unit can start from this accepted baseline.

This creates an `AcceptedIntegrationBaseline`.

```text
AcceptedIntegrationBaseline
  baseline_id
  project_id
  plan_revision
  execution_graph_version
  completed_task_refs[]
  source_target_set
  review_refs[]
  verification_refs[]
  created_from_runs[]
  integration_policy
  created_at
```

## 42.2 Do not define merge cadence by commit count alone

The rule is not `merge every N commits`. Commit count is only a warning signal.

The real boundary is semantic:

```text
reviewable + verified + internally coherent
```

A small task may integrate after two commits. A difficult task may need twenty. But if one work lineage grows so large that Reviewer can no longer evaluate it as one bounded unit, the workflow has already missed an appropriate integration checkpoint.

## 42.3 Merge/promotion policy depends on repository authority

GSA must distinguish:

```text
READY_FOR_INTEGRATION
```

from:

```text
AUTHORIZED_TO_MERGE
```

If repository governance requires Human approval, protected-branch review, or external merge authority, GSA stops at `READY_FOR_INTEGRATION`.

If the approved repository policy explicitly permits automatic integration at that boundary, GSA may perform it.

Agent-authored approval never substitutes for Human merge authority.

## 42.4 Direct-main repositories still need accepted baselines

Some projects may intentionally work directly on `main` instead of feature branches.

In that mode there is no literal branch merge, but the same contract is still required.

The equivalent is:

```text
accepted stable revision
+ durable integration baseline record
```

The WorkCursor should advance from that accepted revision rather than treating every intermediate commit as equally authoritative.

Thus the architectural concept is integration/promotion, not Git merge alone.

## 42.5 New work should start from the latest accepted baseline

After a bounded work slice is accepted:

```text
previous AcceptedIntegrationBaseline
+ approved new Task/JobPack
-> next work lineage
```

This limits how much unreviewed state must be reconstructed after interruption.

It also makes architecture rebaseline easier: accepted baselines are strong candidates for reuse because they already have structured acceptance evidence, but they still require impact review against the new architecture; only work after the last accepted baseline requires detailed salvage classification first.

Previously integrated work may still require impact review, but it does not need to be rediscovered from hundreds of raw commits.

## 42.6 Architecture interruption uses the latest accepted baseline as an anchor

When the Human changes architecture mid-work:

```text
last accepted integration baseline
  -> stable anchor

commits/change sets after baseline
  -> classify through Rebaseline + Salvage
```

This does not mean all work before the baseline is automatically valid under the new architecture.

It means previous acceptance and evidence are already structured, making revalidation cheaper and bounding the delta that must be salvaged.

## 42.7 Integration checkpoints should align with workflow checkpoints when practical

Where practical:

```text
Task/JobPack acceptance
Reviewer PASS
Tester/deterministic gate
integration baseline
```

should occur near the same logical boundary.

Avoid integration churn after every tiny edit, but also avoid carrying several independently complete JobPacks in one unintegrated lineage without a reason.

## 42.8 Integration failure is its own workflow state

If a completed work unit is verified but cannot integrate because of merge conflict, protected branch rules, external source changes, CI policy, cross-repo coordination, or missing Human approval, represent:

```text
READY_FOR_INTEGRATION
INTEGRATION_BLOCKED
INTEGRATING
INTEGRATED
```

Do not send the work back to Coder as if implementation failed unless integration actually requires source adaptation.

## 42.9 Multi-repo integration may be partial but must remain explicit

For MULTI-repo work, repositories may not integrate atomically.

Persist per-repository integration status:

```text
repo A = INTEGRATED
repo B = READY_FOR_INTEGRATION
repo C = BLOCKED
```

The aggregate WorkCursor shows `PARTIAL_INTEGRATION`.

Downstream work may proceed only when the approved topology says the required repository set is ready.

## 42.10 Key invariant

Do not accumulate unbounded unintegrated work.

Prefer:

```text
small coherent implementation slice
-> self-check
-> review
-> required verification
-> accepted integration baseline
-> next slice
```

over:

```text
hundreds of commits
-> one giant review
-> one giant merge
```

This improves resume, rebaseline, review quality, source attribution, and recovery.

---

# 43. Task identity mismatch can block valid Run finalization even when implementation evidence is valid

A control-plane defect was observed during the post-rebaseline Tester continuation.

The bounded baseline-adoption Run had:

- exact source/CI evidence showing the salvaged runtime remained valid;
- a persisted `GOAL_RECHECK` with all active completion criteria `MET`;
- no source mutation in the adoption Run;
- explicit evidence that later commits changed planning/docs only.

However, terminal `run_complete` still rejected the Coder Run with:

```text
CODER_COMPLETION_BLOCKED:
exact GOAL_RECHECK record and its persisted verification ref are required
```

The observed contract mismatch was:

```text
Job task key
  = T2-ADOPT-BASELINE

GOAL_RECHECK.task_id schema
  = UUID only

result
  -> GOAL_RECHECK persisted with task_id = null
  -> verification is real and run-bound
  -> terminal completion cannot recognize it as the exact task recheck
```

This is a control-plane identity problem, not evidence that the implementation failed.

## 43.1 Human-approved progression may be used as a temporary operational exception

When all of the following are true:

1. exact source/runtime evidence is valid;
2. the bounded Task criteria have been rechecked and are `MET`;
3. no unresolved product/code finding remains;
4. the completion blocker is isolated to control-plane bookkeeping/identity;
5. the Human explicitly approves progression;

then the accepted source baseline may be used to begin the next approved Task.

This exception must be explicit and durable. It must not be silently inferred by an agent.

The blocked lifecycle record and the persisted verification remain audit evidence until the control-plane defect is repaired.

Human approval does **not** authorize:

- fabricating a missing PASS;
- deleting the blocked Run;
- weakening verification requirements;
- rewriting source merely to satisfy lifecycle metadata;
- routing around a denied control-plane action through another mutation path.

## 43.2 Deferred fix: unify Task identity across Job, Run, Verification and completion gates

The durable Task contract must use one identity model end-to-end.

Acceptable designs include:

```text
task_id = UUID everywhere
```

or:

```text
task_ref = stable string key
task_uuid = optional canonical UUID
```

but the control plane must not mix:

```text
Job.tasks[].task_id = free-form task key
GOAL_RECHECK.task_id = UUID-only
```

without a deterministic mapping.

The eventual fix should cover:

- `job_prepare` / Job task schema;
- `run_begin.primary_task_id` and `affected_task_refs`;
- `goal_recheck_record.task_id`;
- `run_complete` exact verification lookup;
- Handoff task binding;
- resume/recovery reconciliation.

## 43.3 Completion errors must expose the exact mismatch

A terminal gate should report which field failed to match.

Example:

```text
expected_run_id = ...
observed_run_id = ...

expected_task_ref = T2-ADOPT-BASELINE
observed_task_id = null

verification_ref = ...
verification_persisted = true
verification_result = PASS
```

This prevents a valid implementation Run from becoming stuck behind an opaque generic completion error.

## 43.4 Key invariant

A control-plane bookkeeping defect must not be confused with a product/code failure.

The workflow should preserve both truths:

```text
implementation evidence = accepted
control-plane finalization = defective / deferred repair
```

When Human-approved progression is used, the next Task must start from the exact accepted source target and carry the unresolved control-plane defect as workflow debt rather than pretending it never happened.

---

# 44. Human-approved temporary progression for non-product gate blockers

A workflow gate can fail even when the bounded implementation itself is already acceptable.

Examples include:

- control-plane identity/bookkeeping defects;
- stale or malformed lifecycle metadata;
- CI infrastructure failure;
- formatting-only policy failure after logic has already been independently verified;
- unavailable external service required only by the gate machinery;
- merge/protection/promotion policy that is unrelated to product correctness;
- a verifier integration defect that prevents a valid evidence record from being recognized.

These cases must not be collapsed into:

```text
gate failed
  -> code failed
```

The workflow needs a first-class temporary exception:

```text
HUMAN_APPROVED_TEMPORARY_PROGRESSION
```

This is an operational continuation authority, not a PASS verdict.

## 44.1 Preconditions

Human-approved temporary progression may be requested only when the workflow can show durable evidence for all relevant claims:

1. the exact source target is known;
2. implementation review for that target is acceptable;
3. required code/product tests have independently passed, or the blocked gate is demonstrably outside the unexecuted test surface;
4. no unresolved product/code finding remains;
5. the blocking gate has been classified as a non-product/non-code blocker;
6. the blocker and its repair debt are recorded durably;
7. the Human explicitly approves temporary progression.

If the blocked gate prevented required tests from running and there is no independent PASS evidence for those tests, Human approval must not manufacture that missing evidence.

In that case the workflow may classify the blocker, repair/re-run the gate, or explicitly carry an `UNVERIFIED` limitation, but it must not claim that tests passed.

## 44.2 Temporary progression does not rewrite evidence

The durable record should preserve both facts:

```text
implementation_evidence = ACCEPTED
blocking_gate = FAILED / BLOCKED
progression_authority = HUMAN_APPROVED_TEMPORARY
repair_debt = OPEN
```

It must never be rewritten as:

```text
blocking_gate = PASS
```

unless that gate later actually passes.

The original failing run/log/result remains part of the audit trail.

## 44.3 Scope of the exception must be narrow

A Human-approved temporary progression record should bind at least:

```text
project_id
job_id
task_ref
exact source target/revision
accepted review refs
accepted test/verification refs
blocked gate id/type
block classification
block evidence ref/log
Human approval
allowed next boundary
deferred repair item
created_at
```

The authorization expires when:

- source target changes materially;
- a new product/code finding appears;
- evidence applicability changes;
- the next declared Human/checkpoint boundary is reached;
- or the blocked gate is repaired and rerun.

The exception must not become a permanent global bypass.

## 44.4 Recommended gate classification

Before asking for temporary Human approval, classify the blocker:

```text
PRODUCT_FAILURE
CODE_QUALITY_FAILURE
TEST_FAILURE
VERIFICATION_MISSING
CONTROL_PLANE_DEFECT
CI_INFRA_FAILURE
POLICY_FORMATTING_FAILURE
EXTERNAL_DEPENDENCY_FAILURE
INTEGRATION_POLICY_BLOCK
UNKNOWN
```

Only clearly non-product classes may use temporary progression without first repairing product code.

`UNKNOWN` fails closed.

## 44.5 Current observed incident: formatting-only CI gate blocked T2-ORCHESTRATION

During the current Tester orchestration work, exact target:

```text
main@a9ea713d43884d70cc5d15809c45257c92d0f8d3
```

produced GitHub Actions run:

```text
36745396167
```

with overall CI result:

```text
FAIL
```

The failing repository-check step showed `cargo fmt --check` diffs in:

```text
tests/tester_orchestration.rs
```

The observed failure is therefore currently classified as:

```text
POLICY_FORMATTING_FAILURE
```

not as evidence of a Tester orchestration logic failure.

Important limitation:

The same CI run stopped at the formatting gate, so that run by itself does **not** prove that all downstream tests passed. Any temporary Human-approved progression at this point would still need independent applicable test/review evidence for the exact source target, or the formatting issue should simply be repaired and CI rerun.

This distinction is essential:

```text
format gate failed
!=
logic test failed

format gate failed
!=
tests passed
```

## 44.6 Gate failures should expose whether they invalidate implementation evidence

Every terminal gate result should carry an impact classification such as:

```text
invalidates_implementation = true | false | unknown
invalidates_test_evidence = true | false | unknown
blocks_progression = true | false
repair_owner = coder | workflow | ci | human | external
```

For example:

```text
cargo fmt --check failure
  invalidates_implementation = false
  invalidates_test_evidence = unknown if tests did not run
  blocks_progression = true
  repair_owner = coder/workflow
```

while:

```text
integration test assertion failure
  invalidates_implementation = true
  invalidates_test_evidence = true
  blocks_progression = true
  repair_owner = coder
```

This prevents orchestration from routing every red gate back into product repair.

## 44.7 Deferred architectural requirement

The runtime should eventually support an explicit exception record instead of encoding these cases through ad-hoc `SUPERSEDED` states.

Suggested contract:

```text
TemporaryProgressionException
  exception_id
  project_id
  job_id
  task_ref
  source_target
  accepted_evidence_refs[]
  blocked_gate
  block_classification
  block_evidence_refs[]
  human_approved
  allowed_next_boundary
  expires_on_target_change
  repair_debt_ref
  status = ACTIVE | RESOLVED | EXPIRED
```

Normal orchestration then becomes:

```text
gate BLOCKED
  -> classify blocker
  -> verify accepted implementation/test evidence
  -> Human approval when eligible
  -> persist TemporaryProgressionException
  -> continue only to bounded next boundary
  -> repair blocker later
  -> rerun original gate
  -> resolve exception
```

## 44.8 Key invariant

A temporary Human exception may authorize **progression**, but never fabricate **verification**.

Preserve all three dimensions independently:

```text
product/code correctness
verification status
workflow progression authority
```

A failure in one dimension must not silently rewrite the other two.

---

# 45. Sequential gates can hide downstream blocker classes

Repository verification is ordered. An early red gate may prevent later checks from running, so the first observed blocker must not be treated as a complete diagnosis.

Current T2-ORCHESTRATION sequence demonstrated:

```text
run 36745396167
  -> cargo fmt --check FAIL
  -> tests/check did not fully execute
  -> classification: POLICY_FORMATTING_FAILURE

after test formatting repair

run 36808045800
  -> cargo fmt --check still FAIL in src/registry.rs
  -> classification remains POLICY_FORMATTING_FAILURE

after registry formatting repair

run 36808135785
  -> formatter passes far enough for cargo check
  -> rustc E0597 in src/registry.rs
  -> classification changes to CODE_QUALITY_FAILURE
```

The E0597 defect is a real compile-time implementation defect: a rusqlite Statement was dropped while the mapped-row temporary could still hold a borrow. This blocker is not eligible for Human-approved temporary progression and must be repaired before claiming implementation correctness.

## 45.1 Diagnose incrementally

After each repaired gate:

1. rerun the exact canonical verification pipeline;
2. classify the next observed blocker independently;
3. update eligibility for Human-approved temporary progression;
4. never carry the prior blocker classification forward without new evidence.

## 45.2 Human temporary approval requires evidence past the relevant correctness gates

A non-product gate failure can be temporarily bypassable only when applicable correctness evidence already exists independently.

If the failing early gate prevented compile/tests from running, the workflow cannot infer their result.

Therefore:

```text
early policy gate FAIL
+ downstream checks not executed
-> downstream correctness = UNKNOWN
-> no correctness-based temporary progression yet
```

Once downstream correctness checks are independently PASS, a separate non-product blocker may become eligible for bounded Human-approved progression.

## 45.3 Key invariant

```text
first failing gate != only failing gate
```

Progression decisions must use the deepest actually executed evidence, not the superficial first red status.

---

# 46. Exact-target validation must compare only fields the runtime can authoritatively reconstruct

A T2-ORCHESTRATION regression caused four pre-existing Tester evidence tests to fail with:

```text
Tester target binding is stale for current reviewed prerequisite state
```

The new validator reconstructed the current target from Registry state and compared the entire `TesterTargetBinding` for structural equality.

That was too strict because the current Registry authority can reconstruct:

```text
jobpack_id
prerequisite state
change_set_id
```

but it does not currently persist/reconstruct an authoritative `target_revision` for that prerequisite. Existing valid evidence may carry `target_revision`, while the reconstructed binding returns `None`.

Therefore:

```text
authoritative fields match
+ optional non-reconstructable metadata differs
!= stale target
```

The validator should compare fields according to their authority:

- exact `jobpack_id`;
- exact declared prerequisite state;
- exact current reviewed `change_set_id`;
- `target_revision` only when both the persisted/current authority and supplied binding have authoritative values.

This is not permission to ignore exact-target identity. It is a rule that exactness must be defined against data the runtime can actually prove.

## 46.1 Do not use whole-object equality as an authority check

Whole-object equality is safe only when every field has the same provenance and can be reconstructed from the same authoritative state.

For mixed records containing authoritative identity plus optional observational metadata, validation should be field-aware.

## 46.2 Future repair direction

If `target_revision` becomes required for exact-target safety, persist it first-class in the code/review target state and make Registry able to reconstruct it. Only then should absence/mismatch become a hard stale-target failure.

## 46.3 Key invariant

```text
strict validation
!=
comparison of data the runtime cannot prove
```

Fail closed on authoritative mismatches; do not create false blockers from metadata that currently lacks a reconstruction authority.

---

# 47. Crash-safe Tester resume requires durable execution context, not chat reconstruction

Tester checkpoint recovery must survive process/session loss without depending on the model remembering what happened.

The durable recovery identity must include enough information to reconstruct the exact work boundary:

```text
graph_version
checkpoint_id
attempt_id
target_fingerprint
execution_id
stage
persisted request
replay_safety
deterministic fence
terminal observation when available
```

The current implementation keeps the compact checkpoint/attempt/execution envelope in `latest_checkpoint.stage` and reconstructs execution details from `tester_execution_steps`.

Important stages include:

```text
CHECKPOINT_DUE
EXECUTION_PREPARED
EXECUTION_COMPLETED
ATTEMPT_RECORDED
RECOVERY_REQUIRED
```

The stage transition and its corresponding event/execution mutation must share one database transaction. Otherwise a crash can leave an event claiming progress that `latest_checkpoint` cannot resume, or a checkpoint claiming progress that has no durable evidence behind it.

A restart packet sent to Tester must include persisted completed execution context as well as newly replayed observations. Chat history is not a recovery store.

Key invariant:

```text
restart
-> reconstruct exact durable state
-> reuse completed evidence
-> resume only unfinished eligible work
```

not:

```text
restart
-> ask model what it remembers
-> repeat tests
```

# 48. Replay safety is a runtime contract, not a retry preference

A PREPARED execution means the process may have crashed after durable intent was recorded but before durable completion was recorded.

That uncertainty must be interpreted using explicit replay safety:

```text
OBSERVE_ONLY
  -> exact-fence replay allowed

IDEMPOTENT
  -> exact-fence replay allowed

NON_IDEMPOTENT
  -> never auto-replay when completion is uncertain
  -> RECOVERY_REQUIRED / NEEDS_HUMAN or an explicit recovery adapter
```

The deterministic fence must bind the exact graph, checkpoint, attempt, target fingerprint and request. A restart must not create a new logical execution merely because in-memory state was lost.

Completed persisted steps are reused rather than re-executed. Re-resolving the checkpoint must also preserve a later durable stage such as `EXECUTION_PREPARED`; it must not downgrade the same attempt back to `CHECKPOINT_DUE`.

Key invariant:

```text
same exact work after restart
-> same durable execution identity
```

# 49. Process liveness unknown is not process death

Execution leases prevent duplicate concurrent attempts, but crash recovery also needs to avoid waiting for a long TTL after a process is known to be dead.

For owners encoded as `pid:<n>`, immediate reclamation is safe only when the runtime can positively establish that the process no longer exists.

Therefore:

```text
PID positively alive
  -> keep lease

PID positively dead
  -> reclaim immediately for crash resume

PID liveness unknown / unsupported platform
  -> do not treat as dead
  -> preserve normal stale/TTL policy
```

An implementation where an unsupported platform returns `false` from an `is_alive` helper and the caller interprets `false` as proven death is unsafe: it can let a second process steal a live execution lease.

Use a semantic predicate such as `pid_owner_known_dead` rather than deriving death from the negation of an incomplete liveness probe.

Key invariant:

```text
unknown != dead
```

Fail closed when runtime authority is insufficient.

---

# 50. Temporary progression approval is transition-scoped and must be persisted before the next task

A Human-approved temporary progression is not a reusable project-wide waiver.

The same control-plane defect may recur across multiple Tasks, but each affected boundary has its own exact source target, evidence set, blocked Run and next allowed transition.

Observed sequence:

```text
T2-ORCHESTRATION
  exact implementation evidence PASS
  terminal gate blocked by task identity defect
  Human approves T2-ORCHESTRATION -> T2-RESUME only

T2-RESUME
  exact implementation evidence PASS
  terminal gate blocked by the same task identity defect
  prior approval does not automatically apply
  Human separately approves T2-RESUME -> T2-RETEST
```

The recurrence of the same root defect is useful diagnostic evidence, but it does not broaden authorization.

## 50.1 Persist approval before opening the next Task

The ordering must be:

```text
1. exact CI/test evidence PASS
2. independent Reviewer PASS
3. GOAL_RECHECK PASS
4. normal terminalization attempted
5. non-product control-plane blocker reproduced
6. TemporaryProgressionException / eligibility record persisted
7. explicit Human approval obtained
8. approval persisted with exact transition scope
9. blocked Run terminalized without fake PASS
10. Handoff persisted
11. only then open the next Task
```

Do not open the next Task first and backfill the approval later.

The durable approval record is part of the predecessor boundary, not documentation that can be reconstructed after progression.

## 50.2 Scope must bind both source and destination

A temporary progression approval should include:

```text
source_task_ref
source_run_id
source_target_revision
accepted evidence refs
blocked gate + blocker classification
destination_task_ref
allowed next boundary
Human approval
status
```

Example:

```text
source = T2-RESUME
target = main@70e7782...
destination = T2-RETEST
scope = T2-RESUME -> T2-RETEST only
```

A later transition such as:

```text
T2-RETEST -> T2-CONSUMER-EVIDENCE
```

requires its own eligibility evaluation and, if still blocked by the same defect, its own Human approval.

## 50.3 Repeated blocker does not require repeated diagnosis from zero

When the same defect recurs with the same signature, the workflow may reuse the durable root-cause classification:

```text
CONTROL_PLANE_DEFECT
task_ref / GOAL_RECHECK identity mismatch
```

but it must still re-establish the current Task's eligibility:

- exact target CI/test PASS;
- current Reviewer PASS;
- current GOAL_RECHECK PASS;
- no unresolved code/product finding;
- current terminal gate reproduces the same blocker signature.

This keeps the process efficient without turning one Human approval into a blanket bypass.

## 50.4 Lessons learned are collection, not immediate repair work

When a workflow defect is discovered during an active product/runtime task:

```text
observe
-> record durable lesson
-> preserve evidence
-> finish/continue the approved task path
-> defer architectural repair to a dedicated future task
```

Do not opportunistically patch the workflow/control-plane architecture inside an unrelated Task merely because the defect was observed there.

This prevents scope drift and keeps the active Job's evidence interpretable.

## 50.5 Key invariant

```text
same defect
!=
same authorization
```

Human approval authorizes one explicitly bounded progression. It never silently propagates to later task boundaries.

---

# 51. Temporary progression is a workaround for an incomplete workflow contract, not a target-state feature

Human-approved temporary progression exists only because the current GSA workflow can reach a contradictory state:

```text
implementation evidence = PASS
independent Reviewer = PASS
GOAL_RECHECK = PASS
ready_for_success_handoff = true

but

normal Run terminalization = BLOCKED
```

When this happens because the workflow/control-plane contract cannot represent or correlate the valid evidence correctly, the defect belongs to the workflow architecture.

Temporary Human approval is only a bounded operational escape hatch that prevents the active Job from deadlocking while the workflow defect remains unresolved.

It must not be treated as:

- the normal completion path;
- a permanent governance feature;
- a substitute for a correct Task/Run/Verification contract;
- evidence that the blocked terminal gate is behaving correctly;
- permission to keep requiring Human approval at every healthy task boundary.

## 51.1 Current root defect: Task identity contract is inconsistent across workflow artifacts

The current Job uses stable task references such as:

```text
T2-ORCHESTRATION
T2-RESUME
T2-RETEST
```

but the GOAL_RECHECK/control-plane schema expects a UUID-compatible `task_id`.

The result is a persisted GOAL_RECHECK with:

```text
task_id = null
run_id = exact Coder Run
target_revision = exact source target
result = PASS
all criteria = MET
ready_for_success_handoff = true
```

The terminal Run gate then requires an "exact GOAL_RECHECK record" but cannot match the valid record through the incompatible task identity contract.

Observed effect:

```text
valid implementation
+ valid CI
+ valid Reviewer PASS
+ valid GOAL_RECHECK
-> terminalization blocked by identity mismatch
```

This is a workflow defect, not a product/code failure.

## 51.2 Required future improvement: one canonical Task identity contract

GSA should define one canonical task identity that survives the full lifecycle:

```text
PlanArtifact / Job Task
-> Run
-> Verification
-> GOAL_RECHECK
-> Handoff
-> terminal Run completion
-> resume/recovery
```

Acceptable architectural directions include:

```text
A. UUID task_id everywhere

or

B. stable task_ref everywhere

or

C. stable task_ref + optional immutable UUID,
   with explicit mapping persisted first-class
```

What must not continue is implicit conversion between unrelated identity types or silently dropping a task reference to `null`.

## 51.3 Terminal gate should validate semantic identity, not an accidental schema shape

A successful Coder terminalization should be able to prove:

```text
same project
same Job
same affected Task
same Coder Run
same exact target revision
GOAL_RECHECK result = PASS
required criteria = MET
verification artifact persisted
```

If all of those are true, the terminal gate should not fail merely because one artifact stores the Task as a string ref while another field was typed as UUID.

Conversely, the gate must still fail closed when any semantic binding is ambiguous or stale.

## 51.4 Temporary approval must remain visibly exceptional

While this workflow defect is unresolved:

```text
normal terminalization
-> attempt first

if blocked by known contract defect
-> verify exact evidence
-> classify CONTROL_PLANE_DEFECT
-> request bounded Human temporary approval
-> persist approval
-> progress without rewriting the gate as PASS
```

The existence of this workaround must create repair debt, not normalize the bypass.

A future workflow-improvement task should remove the need for this path for healthy Tasks.

## 51.5 Improvement acceptance criteria

The workflow defect is resolved only when an integration test can demonstrate:

```text
Job task uses canonical identity
-> Coder Run binds same task
-> GOAL_RECHECK persists same task identity
-> Reviewer evidence binds exact target
-> run_complete finds exact verification automatically
-> Run closes normally as COMPLETED/PASS
-> no Human temporary approval required
```

The regression suite should also cover:

- stale GOAL_RECHECK from another target is rejected;
- GOAL_RECHECK from another task is rejected;
- missing verification is rejected;
- superseded task identity cannot complete a newer Run;
- resume/recovery preserves the same canonical task identity.

## 51.6 Key invariant

```text
temporary Human approval
= workaround for workflow contract debt

temporary Human approval
!= target-state workflow design
```

The long-term fix is to make valid evidence composable and machine-verifiable across the full GSA control plane.

---

# 52. Lessons Learned Collector (LLC) is a recorder/classifier for workflow and skill gaps

GSA Local needs a lightweight Lessons Learned Collector, abbreviated **LLC**.

LLC is not a fixer and does not own workflow changes. Its purpose is to preserve reusable lessons when real project execution shows that the current workflow or an Agent skill is insufficient.

LLC may be invoked explicitly by Human with `llc`, or invoked/recommended by another Agent when that Agent detects one of the trigger conditions below.

## 52.1 Primary trigger: an Agent had to find a workaround instead of the canonical path

The strongest workflow-gap signal is:

    canonical path should work
    -> canonical path fails / blocks / cannot express required state
    -> Agent must invent or discover a workaround to continue

When this occurs, the active Agent should treat the workaround as evidence that the workflow contract may be incomplete or inappropriate and call LLC to record it.

Examples include:
- valid code/test/review evidence exists but lifecycle cannot advance;
- manual reconciliation is required because runtime state cannot represent the actual continuation;
- an Agent must bypass, reinterpret, or manually reconstruct a missing contract;
- the workflow forces repeated Human temporary approval for a healthy implementation state;
- orchestration requires information that is available in evidence but has no canonical binding path.

The workaround itself is not automatically accepted as the future design. LLC records the canonical path, where it failed, what workaround was needed, why it was necessary, exact evidence references, and the likely workflow-gap class.

Key signal: `workaround required => inspect for workflow contract debt`.

## 52.2 Secondary trigger: a defect required special detection knowledge that normal Agent review missed

A different class of lesson is an **Agent skill gap**.

Signal:

    normal Agent/Reviewer process did not find the defect
    -> another model, tool, specialist technique, or unusual reasoning path found it
    -> the detection method is reusable

This should be recorded by LLC as a candidate skill/harness improvement rather than immediately modifying the Agent.

Example pattern:

    Reviewer PASS / no finding
    -> real problem remains
    -> Claude or another independent specialist identifies root cause
    -> detection relied on a reusable heuristic or inspection method
    => record AGENT_SKILL_GAP

The lesson should capture which role missed the issue, what evidence was available, what special technique exposed it, why the normal procedure missed it, and what reusable heuristic could later be considered for that Agent.

LLC does not update the skill itself.

## 52.3 LLC classifications

LLC should at minimum distinguish:
- `WORKFLOW_GAP`
- `AGENT_SKILL_GAP`

Optional descriptive sub-classification may include `CONTRACT_GAP`, `STATE_MODEL_GAP`, `ROUTING_GAP`, `EVIDENCE_BINDING_GAP`, `RECOVERY_GAP`, `HUMAN_GATE_GAP`, `REVIEW_HEURISTIC_GAP`, and `TEST_HEURISTIC_GAP`.

These classifications are descriptive only. They do not trigger repair.

## 52.4 Invocation behavior

Human may invoke LLC directly when a workflow problem is observed.

Agents should also know to invoke LLC when they have concrete evidence that one of the triggers occurred.

Expected pattern:

    Agent performs normal work
    -> Agent encounters workaround or unusual detection path
    -> Agent completes/continues the approved task as allowed
    -> Agent calls LLC with evidence
    -> LLC records the lesson
    -> no workflow/source modification occurs

LLC should not be called for every ordinary product bug or expected code failure. It is for reusable deficiencies in GSA workflow, contracts, orchestration, lifecycle/state handling, or Agent review/testing skill.

## 52.5 LLC authority boundary

LLC MAY:
- record the lesson;
- classify it;
- link durable evidence;
- deduplicate it against an existing lesson;
- append additional real-project occurrences;
- note a candidate workflow area or Agent skill that future review should inspect.

LLC MUST NOT:
- change runtime code;
- change workflow contracts;
- modify an Agent skill/harness;
- alter lifecycle state to make the problem disappear;
- approve a workaround;
- create a patch plan;
- decide that a proposed fix is correct;
- block the active Job merely because a lesson was recorded.

Future Planner/Reviewer work decides whether and how collected lessons change GSA.

## 52.6 Recommended lesson record

A recorded LLC item should contain enough context for later independent review:

    lesson_id
    date
    project/job/run/checkpoint
    trigger = WORKAROUND_REQUIRED | SPECIAL_DETECTION_REQUIRED
    classification = WORKFLOW_GAP | AGENT_SKILL_GAP
    canonical_path
    observed_failure
    workaround_or_detection_method
    evidence_refs
    affected_role_or_contract
    reproducibility
    impact
    duplicate_of?
    notes

## 52.7 Key invariants

    LLC records != LLC repairs
    workaround required => likely workflow lesson
    special detection required => likely Agent-skill lesson
    real-project evidence > synthetic confidence for orchestration lessons

The value of LLC is to preserve the signal while the project is still being built and exercised, so later workflow/skill review is based on accumulated real failures rather than memory or isolated anecdotes.


---

# 53. LLC — Valid completed Job can remain IN_PROGRESS because terminal lifecycle transition is blocked

**lesson_id:** LLC-CADGPT-JC694-FINALIZATION-20261001  
**date:** 2026-10-01  
**project:** CadGPT  
**job:** J-C694 / `01a0f6c6-30d6-7454-b5ee-72e6496ac694`  
**trigger:** `WORKAROUND_REQUIRED`  
**classification:** `WORKFLOW_GAP`  
**subclassification:** `EVIDENCE_BINDING_GAP / LIFECYCLE_FINALIZATION_GAP`

## 53.1 Canonical path

The expected terminal path was:

```text
implementation complete
-> Human real-AutoCAD acceptance PASS
-> exact-revision CI PASS
-> Registry Contract PASS
-> independent review / accumulated verification evidence
-> job_transition(IN_PROGRESS -> COMPLETED, PASS)
-> persist terminal Job record
```

J-C694 had reached the product state required for completion.

## 53.2 Observed failure

The final GSA lifecycle transition could not be completed through the canonical control-plane path.

Two attempts to transition J-C694 from `IN_PROGRESS` to `COMPLETED/PASS` were blocked by the control-plane/tool safety layer before the transition record could be produced or persisted.

The implementation itself was not blocked:

- Human confirmed the final CadGPT Beta behavior PASS.
- CadGPT source was on exact revision `5b7c1c6e1eae37eac8a150876a9e6e62df16d850`.
- CadGPT Registry Contract run `36891362644` passed.
- CadGPT Core CI run `36891362542` passed.
- The final TBH contract was verified in real AutoCAD: successful `tbhloader.lsp` load enables the TBH toolkit for that drawing; failed load leaves it OFF.

Despite this, the durable Job record remained `IN_PROGRESS`.

## 53.3 Workaround used

No lifecycle state was falsified.

The Agent reported the distinction explicitly:

```text
functional/product state = PASS
GSA lifecycle record = still IN_PROGRESS
reason = canonical terminal transition blocked by control-plane
```

The Human then accepted CadGPT Beta as complete based on the real implementation/test evidence rather than pretending that the blocked GSA lifecycle mutation had succeeded.

This is a temporary operational workaround, not the desired workflow.

## 53.4 Why this is a workflow lesson

This matches LLC's strongest trigger:

```text
valid implementation + Human evidence + CI evidence exist
-> canonical lifecycle close path cannot advance
-> Agent must reconcile real completion separately from control-plane state
```

A healthy Job should not remain permanently `IN_PROGRESS` when all completion evidence is valid merely because the terminal transition machinery cannot express or authorize the close.

The workflow therefore needs a first-class recovery/finalization contract for this condition.

## 53.5 Affected contract

Primary affected areas:

- Job terminal lifecycle transition;
- completion-evidence binding;
- control-plane mutation authorization;
- recovery from a blocked `job_transition`;
- reconciliation between product truth and durable GSA lifecycle truth.

This lesson is closely related to the earlier terminalization/evidence-binding lesson, but this occurrence is specifically at **Job finalization**, after the product and Human gate had already passed.

## 53.6 Reproducibility

Observed in real project execution on CadGPT J-C694:

```text
Job lifecycle = IN_PROGRESS
Human acceptance = PASS
Core CI = PASS
Registry Contract = PASS
completion evidence supplied
-> job_transition to COMPLETED/PASS blocked
-> retry also blocked
-> Job remains IN_PROGRESS
```

## 53.7 Impact

- project work can be genuinely finished while GSA still reports an active Job;
- future Agents may incorrectly believe more implementation work remains;
- dashboards/task summaries can drift from the actual product state;
- Humans may be forced to grant semantic completion outside the canonical lifecycle path;
- repeated occurrences would make lifecycle state less trustworthy.

## 53.8 Future review target

LLC records this only; it does not prescribe the fix.

Future workflow review should determine how a terminal Job can be safely reconciled when:

```text
completion evidence is valid
+ Human acceptance is explicit
+ exact target revision is known
+ normal job_transition is blocked by control-plane mechanics
```

The final mechanism must remain fail-closed for stale, ambiguous, or cross-Job evidence, but it should provide a bounded canonical recovery path instead of leaving a valid Job permanently `IN_PROGRESS`.

## 53.9 Key invariant

```text
real completion evidence must not be rewritten as failure
blocked lifecycle mutation must not be rewritten as PASS
a healthy workflow needs a canonical reconciliation path between the two
```



---

# 54. LLC — Agent selection currently forces workflow execution, and Planner workflow tool invocation fails on real Ollama models

**lesson_id:** LLC-GSA-LOCAL-AGENT-ROUTING-PLANNER-TOOL-20261002  
**date:** 2026-10-02  
**project:** Gsa-local  
**job:** J-7222 / Phase 12 runtime validation context  
**trigger:** `WORKAROUND_REQUIRED`  
**classification:** `WORKFLOW_GAP`  
**subclassification:** `ROUTING_GAP / CONTRACT_GAP / LOCAL_MODEL_INTEGRATION_GAP`

## 54.1 Canonical path

Interactive Agent mode should allow ordinary Human-Agent conversation/intake without automatically creating a workflow transition.

For an actual planning request, the expected path is:

```text
Human discusses / clarifies with Planner
-> planning intent is established
-> Planner invokes the required planning workflow tool
-> Reviewer
-> Local CR when required
```

Selecting the Planner role should not itself mean that every subsequent user message is a planning workflow execution request.

## 54.2 Observed failure A — ordinary conversation is routed into the planning workflow

Real local runtime test on MacBook, repository state beginning from:

```text
main@763b20c
./scripts/ci.sh -> CI_EXIT=0
```

The user selected:

```text
/agent
2. Planner
```

A real planning prompt produced:

```text
Planning workflow: Planner -> Reviewer -> Local CR
error: Planner did not call the required workflow tool
```

After that, an ordinary conversational input:

```text
hello?
```

also immediately produced:

```text
Planning workflow: Planner -> Reviewer -> Local CR
```

This shows that the current interactive routing conflates:

```text
active agent identity
```

with:

```text
workflow execution intent
```

The defect is architectural because it prevents normal clarification/intake conversation with an Agent and can cause lifecycle/workflow machinery to activate on non-workflow messages.

## 54.3 Observed failure B — real Planner workflow does not invoke the required workflow tool

The same real planning prompt was tested against two different locally installed Ollama models that both advertise tool capability:

```text
qwen38t:latest
gemma4u:latest
```

Both runs reached:

```text
Planning workflow: Planner -> Reviewer -> Local CR
```

and both failed with:

```text
error: Planner did not call the required workflow tool
```

Because the same failure reproduced across two distinct tool-capable models, this occurrence should not be treated as evidence that one specific model is defective.

The integration path requiring future inspection includes:

- Agent/workflow intent routing;
- Planner system/harness instructions;
- tool schema supplied to Ollama;
- Ollama tool-call response shape;
- local response parsing;
- required-workflow validation.

LLC does not decide which of these is the root cause.

## 54.4 Workaround / current operational decision

No workflow gate was weakened and no prompt workaround was accepted.

The temporary operational decision is:

```text
record the real-project failure
-> do not repair it inside the current Phase 12 resume task
-> continue the already approved Phase 12 work
-> return to Agent/Ollama interaction repair in a dedicated later task
```

Testing additional models was stopped because repeated model swapping would not resolve the routing defect and had already reproduced the Planner tool-call failure across two tool-capable models.

## 54.5 Why this is a workflow lesson

This matches the LLC trigger because the canonical interactive path cannot currently support both:

```text
normal Agent conversation
and
explicit workflow execution
```

as distinct operations.

It also exposes a second contract gap where a correctly routed planning task cannot progress because the required tool invocation is not observed by the runtime.

These failures were not discovered by repository CI. They appeared only when GSA Local was exercised against a real Ollama service and real locally installed models.

## 54.6 Reproducibility

Environment observed:

```text
GSA Local interactive shell starts successfully
Ollama API reachable on localhost:11434
/model discovers 5 installed models
/agent exposes General, Planner, Job Builder, Reviewer, Coder, Tester, CR
qwen38t:latest -> Planner required-workflow failure
gemma4u:latest -> same Planner required-workflow failure
Planner + "hello?" -> planning workflow activates
```

The source baseline had already passed the repository canonical CI on the same MacBook before this runtime interaction test.

## 54.7 Impact

- ordinary clarification/chat with a selected specialist Agent is not possible without unintended workflow activation;
- workflow transitions can be attempted for messages with no workflow intent;
- local tool-capable models cannot currently complete the observed Planner contract;
- synthetic/unit CI can report green while the real Ollama-Agent path remains unusable;
- debugging by relaxing the required-tool gate would risk hiding the actual integration defect.

## 54.8 Future review target

LLC records only the failure and evidence.

A future dedicated workflow task should determine the correct contract for:

```text
conversation/intake
vs
workflow invocation
```

and separately trace the exact Planner-to-Ollama tool-calling path before any gate behavior is changed.

The repair should be validated with a real local model, not only mocked tool responses.

## 54.9 Key invariants

```text
selecting an Agent != invoking that Agent's workflow

ordinary conversation must not create lifecycle work by default

required workflow tool missing
!= permission to bypass the required workflow tool

real local-model integration evidence must be part of acceptance
```


---

# 55. LLC — Branch/commit fragmentation causes source-truth drift and implementation-plan incompleteness

**lesson_id:** LLC-GSA-ONLINE-SOURCE-TRUTH-BRANCH-RECONCILIATION-20261004  
**date:** 2026-10-04  
**project:** Gsa-local  
**trigger:** `WORKAROUND_REQUIRED / SOURCE_RECONCILIATION_REQUIRED`  
**classification:** `WORKFLOW_GAP`  
**subclassification:** `SOURCE_TRUTH_GAP / BRANCH_RECONCILIATION_GAP / PLAN_IMPLEMENTATION_DRIFT`

## 55.1 Canonical path

An active Job needs one explicit **source-of-truth branch/ref**.

Accepted implementation work must converge into that source-of-truth continuously:

```text
task starts from source_truth_ref
-> Coder may use a temporary working branch if required
-> accepted change is reviewed/verified
-> accepted commit is integrated into source_truth_ref
-> source_truth_ref advances
-> next task/checkpoint starts from that exact ref
```

A task is not operationally complete while accepted code exists only on an unmerged side branch.

## 55.2 Observed failure

During Gsa-local work, multiple development lines were left alive while `main` continued advancing.

Observed repository state during reconciliation:

```text
main = active executable source truth

upgrade-v3
  -> no commits ahead of main
  -> 207 commits behind main

migrated-plan
  -> 18 commits ahead of its old merge-base
  -> 207 commits behind current main
  -> open WIP PR still present
```

The 18 commits were not one coherent change that could safely be merged.

They contained both:

- older Tester orchestration already superseded by richer Phase-12/main architecture; and
- later grounded Tester capability checks that were still useful and had never reached main.

Therefore recovery required a manual branch/commit archaeology pass:

```text
enumerate branches
-> compare each branch to main
-> inspect every unique commit
-> map old implementation to current architecture
-> port only surviving logic
-> reject superseded logic
-> close/reset stale integration branches
```

This is expensive and unsafe work that should not be required at the end of a Job.

## 55.3 Impact

Branch/commit fragmentation creates several workflow failures:

- accepted code can be absent from the branch used by later tasks;
- implementation can become incomplete relative to the approved plan;
- Reviewer/Tester may verify a different source line from the one later considered canonical;
- later Agents cannot know whether an unmerged commit is required, superseded, experimental, or abandoned;
- large branch divergence makes direct merging unsafe even when some commits remain useful;
- final cleanup becomes semantic code archaeology instead of deterministic reconciliation;
- source history becomes a poor substitute for durable workflow state.

The practical symptom is:

```text
plan says task X was implemented
but source_truth_ref does not contain all of task X
```

That must be treated as workflow failure, not routine cleanup.

## 55.4 Required workflow contract

Every active Job should carry an explicit:

```text
source_truth_ref
source_truth_revision
```

For normal Gsa-local development the source truth is:

```text
branch = main
```

unless Human explicitly establishes another integration branch for the whole Job.

Temporary branches may exist, but they are subordinate working refs, never competing source truths.

At each task/checkpoint boundary the control plane should perform a **Branch Reconciliation Gate**:

```text
for every Run that produced accepted source:
    output revision must be reachable from source_truth_ref

for every temporary branch:
    accepted commits -> integrated
    superseded/rejected commits -> explicitly classified
    unresolved commits -> BLOCK next task/checkpoint

source_truth_ref HEAD
    -> exact input revision for next task
```

## 55.5 Accepted-code invariant

A commit is not considered integrated implementation evidence merely because it exists in the repository.

Required invariant:

```text
accepted source commit
=> reachable from source_truth_ref
```

If not reachable:

```text
task status != complete
checkpoint progression = blocked
```

This prevents later tasks from silently starting from incomplete source.

## 55.6 Plan/source completeness check

Before advancing a task or milestone, GSA should compare:

```text
approved implementation plan
+ completed task outputs
+ accepted Run output revisions
against
current source_truth_ref
```

The gate should answer:

```text
all accepted implementation present?
all planned task outputs represented?
any accepted commits stranded on side branches?
any source changes present that are not attributable to the active plan/Run lineage?
```

A mismatch becomes `SOURCE_RECONCILIATION_REQUIRED`, not an implicit assumption that the latest branch is correct.

## 55.7 Temporary branch lifecycle

A temporary branch must have:

```text
owner Run/Task
base source_truth_revision
merge target = source_truth_ref
status = ACTIVE | INTEGRATED | SUPERSEDED
```

When its accepted changes are integrated:

```text
temporary branch -> INTEGRATED
PR -> merge/close as appropriate
branch may be deleted/reset
```

When its work is obsolete:

```text
temporary branch -> SUPERSEDED
unique commits explicitly classified as rejected/superseded
PR closed
branch removed/reset
```

Long-lived anonymous WIP branches are not durable workflow state.

## 55.8 Reproducibility signal

The LLC trigger should fire when any of these occur:

- a side branch is ahead of source truth after its owning task is considered complete;
- a later task starts before prior accepted commits are reachable from source truth;
- Humans/Agents must manually search old branches to discover missing implementation;
- a plan-completion review finds code claimed as done but absent from the canonical branch;
- multiple branches are treated as plausible versions of current source;
- an open WIP PR survives beyond the task/milestone that created it without an explicit unresolved dependency.

## 55.9 Key invariants

```text
one active Job -> one explicit source-of-truth branch/ref

accepted code must converge to source truth continuously

task complete
=> all accepted task commits reachable from source truth

next task input
= exact current source-truth revision

temporary branch != alternate source of truth

unmerged accepted commits
= workflow block, not end-of-project cleanup
```

---

# 56. LLC — Individual Jobs do not consistently inherit a mandatory context-scan/workflow procedure even though Planner already builds bounded repository context

**lesson_id:** LLC-GSA-WORKFLOW-TEMPLATE-SELECTION-CONTEXT-SCAN-20261005  
**date:** 2026-10-05  
**project:** Gsa-local  
**trigger:** `WORKAROUND_REQUIRED / SPECIAL_DETECTION_REQUIRED`  
**classification:** `WORKFLOW_GAP`  
**subclassification:** `EXECUTION_TEMPLATE_GAP / CONTEXT_DISCOVERY_GAP / JOBPACK_WORKFLOW_BOUNDARY_GAP`

## 56.1 Observed failure pattern

GSA already has strong planning and execution structures, but the actual repository-discovery procedure is not consistently inherited by individual work stages.

The practical symptom is:

```text
Planner performs bounded repository context discovery
-> Plan / Job Pack is created
-> later individual Job/role execution begins
-> Coder / bug-fix / Tester path may inspect only the immediately obvious files
-> broader related source tree / callers / tests / contracts are not consistently scanned
-> latent relationship gaps are discovered later by Reviewer, CI, another model, or Human
```

This creates a recurring workflow smell:

> the plan knows more about repository structure than the individual execution stage that is supposed to implement or verify it.

The issue is not that every Agent must read the whole repository. The issue is that the runtime does not currently guarantee one declared, reusable **execution procedure** that tells each work type what context-discovery steps are mandatory before mutation or verdict.

## 56.2 Source scan — existing mechanisms that must not be duplicated

A source scan of current Gsa-local shows several existing layers.

### Planner already has a bounded relative-tree context primitive

`PlanningWorkflow::run()` calls:

```text
build_project_context(project_root)
```

before invoking Planner.

The helper:

- canonicalizes the project root;
- recursively enumerates relative file paths;
- skips `.git`, `.gsa`, `target`, `node_modules`, `.next`, `dist`, `build`;
- skips sensitive files such as `.env*`, `.npmrc`, credentials and private keys;
- bounds the number of paths;
- stores them as a relative project context.

Therefore **relative-tree discovery already exists as a real runtime primitive**. A future workflow-template design should reuse/generalize this primitive rather than inventing a second scanner.

### Job Pack already defines work decomposition, not execution procedure

Current Job Pack contract contains fields such as:

```text
goal
todo_ids
required_inputs
expected_outputs
acceptance
verification_hints
```

Job Pack answers:

```text
WHAT bounded work is active?
WHAT must it produce?
WHAT acceptance must be met?
```

It does not fully answer:

```text
HOW must this class of work inspect the repository before acting?
WHICH context scan is mandatory?
WHICH mutation/review/test sequence must be followed?
WHICH evidence must be collected at each procedural stage?
```

Therefore the proposed workflow-template concept should **not replace Job Pack** and should not duplicate its goal/TODO/input/output/acceptance responsibilities.

### GSA already has role-specific workflow/state-machine logic

Current source already contains dedicated workflow machinery including:

- `PlanningWorkflow`;
- `CodingWorkflow`;
- Reviewer stages inside Coding workflow;
- Internal Fix / corrective coding behavior;
- `CodeCrWorkflow`;
- Tester-related workflow/state/evidence paths;
- role harnesses for Planner, Job Builder, Coder, Reviewer, Tester, Local CR.

These are important prior art. A new “workflow template” layer must not blindly add another parallel state machine with the same responsibility.

## 56.3 Gap after the scan

The missing abstraction appears to be **between Job Pack and Agent freedom**.

Current behavior is approximately:

```text
Job Pack selects bounded work
+ Harness defines role behavior
+ Workflow code controls some transitions
+ Agent still decides much of the concrete inspect/act procedure
```

For example, Coder is told to:

```text
Inspect live source before writing.
```

Reviewer is told to inspect source with read tools as needed.

Those instructions are valid, but they are not equivalent to a deterministic reusable procedure such as:

```text
FEATURE_CODING
1. bounded relative-tree scan
2. identify direct target + callers + tests + contracts
3. classify relevant / irrelevant relationships
4. inspect exact affected files
5. mutate
6. inspect diff
7. run declared verification
8. goal recheck
```

Without such a procedure, two executions of the same Job class can perform materially different discovery depth even when using the same Job Pack and role.

## 56.4 Candidate direction for future review — predefined workflow templates

A possible future architecture is to require every execution to declare/select one canonical workflow template **before work starts**.

Examples to investigate:

```text
PLANNING
FEATURE_CODING
BUG_FIX
TESTING
CODE_REVIEW
INTERNAL_FIX
RECOVERY / RESUME
```

Conceptually:

```text
active Job Pack
-> classify work intent
-> select one registered workflow template
-> runtime provides that template's mandatory discovery / action / evidence stages
-> Agent executes inside the selected workflow
-> Job Pack still supplies the bounded goal and acceptance
```

The Agent should not silently invent a new work procedure per turn.

However, LLC does **not** yet conclude that these exact template names, stages, or a new registry are the correct implementation.

Before implementation, a dedicated architecture scan must map the proposal against:

- existing `PlanningWorkflow`;
- `CodingWorkflow` and Internal Fix loop;
- Tester workflow/checkpoints;
- `CodeCrWorkflow`;
- Planner / Job Builder / Coder / Reviewer / Tester harness contracts;
- Job Pack fields and execution graph;
- Test Checkpoints / Evidence Requirements;
- Resume/Recovery work already recorded in this lessons ledger.

Any proposed template layer that duplicates one of those mechanisms should be rejected or collapsed into the existing owner.

## 56.5 Required architectural question

Future design should explicitly answer:

```text
Job Pack = WHAT work?
Workflow Template = HOW this class of work must execute?
Harness = WHO/role behavior?
Execution Graph = WHEN/ordering/dependencies?
Checkpoint/Evidence = WHAT proof is required?
```

If these ownership boundaries cannot be made non-overlapping, adding Workflow Templates would create another abstraction layer rather than fix the workflow gap.

## 56.6 Context scan should be reusable, bounded, and workflow-owned

The Planner's existing relative-tree scan is evidence that GSA already knows how to produce safe bounded repository context.

A future review should test whether the correct direction is:

```text
one shared Project Context / Relationship Scan primitive
-> parameterized by selected workflow
-> available to Planner / Coder / Bug Fix / Tester / Reviewer as appropriate
```

rather than every Agent independently deciding whether to run `list/search/read`.

The workflow should define the minimum scan obligation.

Examples:

```text
Planning:
  broad bounded tree + architecture/source evidence

Feature coding:
  affected subtree + callers/consumers + tests/contracts

Bug fix:
  reproduction path + implementation owner + callers + regression tests

Testing:
  changed surface + acceptance contract + relevant test/CI configuration

Review:
  exact change set + affected relationships + tests/contracts
```

These are **review hypotheses**, not yet accepted runtime requirements.

## 56.7 Why this is a workflow lesson

This is not a one-off missed file.

The reusable failure pattern is:

```text
Agent has a valid bounded Job
but no mandatory selected execution procedure
-> context discovery varies by model/run
-> important related files may be skipped
-> later review/testing must recover missing context
```

The fact that Planner already has an automatic relative-tree scan while later work stages do not consistently inherit an equivalent context contract demonstrates that repository-awareness is currently **stage-specific rather than workflow-systematic**.

## 56.8 Future validation target

A future dedicated correction task should first perform an overlap inventory, then prove the chosen design on real repositories.

Minimum negative cases:

1. A Feature Coding Job where the target file has a direct caller in another directory.
2. A Bug Fix where the visible failing function is not the actual owner of the defect.
3. A Tester run where the relevant CI/config file is outside the changed subtree.
4. A Reviewer run where the diff looks locally correct but violates a neighboring contract.
5. A workflow resumed midway without repeating unnecessary broad discovery.
6. A tiny isolated Job where the workflow remains bounded and does not over-scan.
7. A Job Pack whose acceptance is complete but whose selected workflow has not completed its mandatory evidence stages.

The design should demonstrate that mandatory workflow selection improves consistency without turning every Job into a full-repository scan.

## 56.9 Key invariants / hypotheses to preserve during future design

```text
Job Pack != Workflow Template

workflow template must not duplicate Job Pack goal/input/output/acceptance ownership

workflow template must not duplicate existing workflow state machines without an explicit consolidation plan

Agent must not silently invent its own execution procedure when a canonical workflow applies

context discovery should be runtime/workflow-owned, bounded, and reusable

broad scan is not mandatory for every Job
but the selected workflow must define the minimum required scan

existing Planner relative-tree primitive should be reused/generalized before creating a new scanner

LLC records the gap only
-> no workflow/runtime change is authorized by this lesson alone
```

---

# 57. LLC — V2_STABLE Tester Run terminalization can accept PASS with no TEST Verification

**lesson_id:** LLC-GSA-LOCAL-TESTER-RUN-COMPLETION-NO-VERIFICATION-20261010  
**date:** 2026-10-10  
**project:** Gsa-local / J-9067 (UAR-2B)  
**trigger:** `SPECIAL_DETECTION_REQUIRED / CANONICAL_GATE_GAP`  
**classification:** `WORKFLOW_GAP`  
**subclassification:** `EVIDENCE_BINDING_GAP / FALSE_GREEN_GATE / EXACT_TARGET_MISMATCH`

## 57.1 Evidence observed during drift reconciliation

- The canonical GSA `run_complete` **prepare-only** call was made for the older Tester Run `R-8548` (`01a114cf-e807-7f32-bfc2-facdec908548`, execution contract `V2_STABLE`, input `target_revision=a1b96538...`).
- The request proposed `lifecycle=COMPLETED`, `result=PASS` with **no** `run_verification_snapshot`, **no** `verification_refs`, and no matching durable current-target TEST Verification.
- The API **returned a proposed `COMPLETED/PASS` Run update** instead of rejecting missing TEST evidence. This was a returned proposal, **not a persisted update**. The assistant intentionally did **not** call the Memory mutation path, and the Tester Run remains `IN_PROGRESS`.
- Contrast: `run_complete` for UAR-2B Coder Run `R-AC9E` rejected a proposed successful completion with `CODER_COMPLETION_BLOCKED: exact GOAL_RECHECK record and its persisted verification ref are required for successful completion.`
- The actual real Ollama probe **did PASS**, but at a later PR commit `6475f1e`; its tree equals squash-`main@75cf522d`. This is source-content equivalence, **not evidence that the historical Run target `a1b96538` passed exact-target testing**.

## 57.2 Why this is a workflow issue

A caller can receive a plausible successful Tester terminal Run record without supplying evidence binding, while Coder success already requires exact GOAL_RECHECK. A passing CI run or a successful real test on a later commit must not silently satisfy an older Tester Run. `run_complete` preparation success is **not** independent confirmation of product quality or lifecycle authority.

## 57.3 Deferred review direction (not an immediate fix)

A dedicated GSA workflow-contract review should assess the existing V2_STABLE and NEXT completion paths, compare how Reviewer/Tester Verification is persisted and consumed, and enforce *the intended evidence requirements at the appropriate declared boundary*. Any change must preserve compatibility with legitimate historical completed Runs and must not introduce fake retroactive target binding. Test both a missing-verification negative case and a true exact-target positive case. Record the observed rejection/acceptance and ensure no second terminalization after a blocked attempt.

**LLC records only.** Do not treat this lesson as authorization to weaken UAR-2B gates, rebind immutable target revisions, or silently promote stale Tester Runs.
