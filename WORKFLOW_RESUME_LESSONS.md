# GSA Workflow Resume / Recovery — Lessons Learned

## Purpose

This document captures workflow-level lessons from real interrupted and recovered GSA sessions.

It intentionally does not serve as a bug log. The goal is to identify failure patterns that can make work impossible to resume, resume from the wrong point, duplicate work, bind evidence to the wrong target, remain blocked after the underlying work is valid, or silently advance into work the Human did not authorize.

The findings compare GSA Online control-plane behavior with GSA Local runtime behavior and define missing contracts for both.

## Evidence base

Observed patterns came from durable GSA Project history and the current GSA Local implementation.

Representative durable history includes recovery/repair sequences around J-177F / T-EXECUTION:
- interrupted source changes existed before durable Run lineage was reconstructed;
- Run identity required repair;
- target revision/ref required rebinding;
- GOAL_RECHECK references were rebound/normalized before completion gates accepted the Run;
- source progress continued across long sessions while durable Run state lagged behind;
- stale planning evidence required cleanup after Human decisions changed.

The current Online control plane exposes project lifecycle resume, run_begin, goal_recheck_record, run_complete and handoff generation, but no first-class primitive that resumes an interrupted execution Run.

The current Local runtime persists checkpoints, workflow state and an execution lease, but Planner/Coder entry paths still start workflows by resetting state instead of first deciding whether an existing workflow must be resumed.

## 1. Core lesson: resume is a state transition, not a prompt

A Human saying “continue” must not require the model to infer the execution point from conversation history.

The runtime must resolve a Resume Decision from durable state plus live source.

Required output concept:

    RESUME_DECISION
      project
      job
      task / jobpack
      run lineage
      workflow stage
      exact input target
      current live source target
      outstanding gate
      allowed next role/action
      recovery classification

Conversation history may help explain intent, but must not be the authority for execution position.

## 2. Failure pattern: source state and durable Run lineage diverge

A long session can stop after source commits exist but before the matching Run/Handoff/Verification records are durably closed.

On the next session, two truths coexist:

    SOURCE
      contains newer legitimate work

    PROJECT MEMORY
      still points to an earlier/incomplete Run

Manual recovery then has to decide whether those commits are:
- legitimate interrupted same-scope work;
- obsolete work from a superseded architecture;
- unrelated work;
- unknown.

Both Online and Local need a first-class reconciliation phase before resuming interrupted work.

Suggested classification:

    DURABLE_KNOWN
    ADOPTABLE_SAME_SCOPE
    SUPERSEDED_BY_NEW_PLAN
    OUT_OF_SCOPE
    UNKNOWN_REQUIRES_HUMAN

A recovered source commit must never be silently adopted merely because it is newer.

Persist a recovery record containing last durable revision, live revision, observed delta, classification, scope basis and decision.

## 3. Failure pattern: Run identity can become repairable data

Durable history required explicit Run identity repair when a persisted Run record/path and intended generated Run identity diverged.

Run identity must be immutable and runtime-owned.

The agent should never repair run_id, handle, memory path or parent_run_id linkage by manually editing JSON.

Online should create/persist Run identity atomically or reject path/content identity mismatch at write time.

Local should use runtime-generated persisted workflow/run identity for every resumable execution unit.

## 4. Failure pattern: one field is being used for two different revisions

During an Internal Fix, a Run starts from one source revision and produces a later revision. Durable history then required manual rebinding of target_revision and target_ref before completion gates accepted the Run.

The workflow is mixing:

    INPUT TARGET
      what the Run started from

    RESULT TARGET
      what the Run produced / verified

These must be separate immutable concepts.

Recommended Run contract:

    input_target:
      ref
      revision

    result_target:
      ref
      revision

Rules:
- input_target is immutable after Run start;
- result_target is assigned only from observed source mutation/exact source evidence;
- Reviewer and Goal Recheck bind to result_target;
- recovery compares live source against both.

Manual “change target_revision to latest” should disappear.

## 5. Failure pattern: completion gate depends on manually assembled reference wiring

T-EXECUTION history required multiple durable edits to attach GOAL_RECHECK refs, replace them with a normalized/final one, normalize target_ref and bind the exact verification record expected by the completion gate.

The product work can be valid while the Run remains blocked because durable references were not assembled exactly as expected.

A gate should validate semantic state, not require the agent to hand-wire bookkeeping the runtime can derive.

Introduce a canonical per-Run verification slot:

    Run
      latest_goal_recheck
      latest_code_review
      latest_test_verification
      latest_required_ci

When a newer exact-target Verification supersedes an older one, runtime updates the canonical pointer transactionally.

Append-only history remains preserved; canonical pointers identify current gate evidence.

## 6. Failure pattern: Online has Project resume, not Work resume

Online GSA exposes:
- project_resume: PAUSED Project to ACTIVE Project;
- run_begin: creates a new Run;
- run_complete: closes an existing Run.

There is no first-class run_open, run_resume, interrupted-run recovery, open-run resolver or resume-decision primitive.

After a chat/session interruption the model must reconstruct continuity itself and often creates a child/new Run to represent recovery. The policy is implicit and inconsistent.

Add an Online primitive conceptually like:

    run_recover(project_id, job_id, live_source_revision)

Return one of:

    RESUME_EXISTING
    START_RECOVERY_CHILD
    START_NEW_RUN
    BLOCKED_RECONCILIATION_REQUIRED
    ALREADY_TERMINAL

and include active/open Run, parent lineage, task scope, last durable source target, live source target, missing durable artifacts and allowed next transition.

## 7. Failure pattern: Local restart currently resets Coder workflow

Current CodingWorkflow::run() begins with begin_code_workflow(...).

begin_code_workflow resets:
- change_set_id;
- coder_attempts;
- reviewer_attempts;
- workflow status;
- checklist completion;
- TODO state.

This is correct for a new coding workflow but unsafe for resume.

The Local app persists code_workflow_state, but execution entry does not first inspect it and decide whether to resume.

Replace unconditional begin with resolve_code_entry().

Decision table:

    no state for current Job Pack
      -> BEGIN_NEW

    CODER
      -> RESUME_CODER from durable source state

    REVIEWER
      -> RESUME_REVIEWER for exact persisted change set

    INTERNAL_FIX
      -> RESUME_INTERNAL_FIX with persisted findings/change set

    REVIEW_PASS
      -> do not rerun Coder; continue to next declared gate

    PAUSED
      -> apply explicit recovery policy

A restart must not zero attempts/checklists or erase current change-set identity.

## 8. Failure pattern: Local Planning workflow has the same restart hazard

PlanningWorkflow::run() unconditionally calls begin_plan_workflow(), which resets current revision and review attempt counters to a new PLANNING state.

Planning needs resolve_planning_entry() and must resume from PLANNING, REVIEWER, LOCAL_CR, INTERNAL_FIX, PAUSED or APPROVED without creating a fresh revision unless the contract actually requires one.

## 9. Failure pattern: in-memory execution context is richer than durable resume context

Some coding continuity lives in process memory:
- tool-runtime mutation journal;
- model message history;
- current execution round;
- transient previous checkpoint objects.

Registry persists important workflow state and code checkpoints, but a fresh process cannot necessarily reconstruct the exact active agent packet without additional resolver logic.

Anything required to choose the next safe action after restart must be durable or deterministically reconstructible.

Do not persist chat transcript/hidden reasoning.

Persist only minimal execution facts:
- active role/stage;
- exact jobpack/task;
- input/result target;
- change_set identity;
- latest findings;
- canonical verification refs;
- attempts;
- mutation/evidence digest;
- recovery class.

## 10. Failure pattern: environment limitation can accidentally look like success

A Goal Recheck may find source criteria MET while required executable verification remains unavailable.

If the overall record is still labelled PASS, downstream workflow can treat “source logic looks correct” as equivalent to “required verification completed.”

Required verification availability is part of completion semantics.

Recommended split:

    GOAL_STATUS
      MET / PARTIAL / NOT_MET

    VERIFICATION_STATUS
      VERIFIED / UNVERIFIED / BLOCKED / NOT_APPLICABLE

    GATE_STATUS
      READY / NOT_READY / NEEDS_HUMAN

No terminal success when required verification is UNVERIFIED or BLOCKED.

This rule must be the same Online and Local.

## 11. Failure pattern: stale planning evidence survives Human decisions

Some Jobs required explicit cleanup because prose/source evidence from an older decision remained in the latest Job revision.

Evidence lifecycle should be structured, not maintained by deleting prose manually.

Recommended evidence metadata:
- evidence_id;
- valid_for_revision;
- status: CURRENT / SUPERSEDED / INVALIDATED;
- superseded_by;
- reason.

Latest Plan/Job views should consume CURRENT evidence only while preserving history.

This prevents old architectural assumptions from being reintroduced during resume.

## 12. Failure pattern: Handoff can be mistaken for authorization to start next work

A Handoff identifies expected next role/outcome, but it must not imply the next Task/phase has started.

Separate:

    HANDOFF_AVAILABLE

from:

    NEXT_WORK_ACTIVATED

Add activation_policy:
- EXPLICIT_START;
- AUTO_CONTINUE.

For phase/task boundaries, default to EXPLICIT_START unless the approved plan explicitly allows automatic continuation.

## 13. Unified resume protocol

Both Online and Local should implement the same conceptual protocol.

Step A — Resolve durable authority:
- Project lifecycle;
- Job lifecycle;
- approved plan / graph;
- current task or Job Pack;
- open/terminal Runs;
- latest canonical verification;
- handoff;
- checkpoint/workflow stage.

Step B — Resolve live source truth:
- registered repository;
- live branch/ref;
- live source revision;
- delta since last durable source target.

Step C — Reconcile:

    durable == live
      -> normal resume

    live ahead, same scope and attributable
      -> recovery/adoption record

    live ahead, superseded architecture
      -> do not adopt; route to rebaseline

    live ahead, unknown ownership/scope
      -> BLOCKED_RECONCILIATION_REQUIRED

    durable ahead / source missing
      -> BLOCKED_SOURCE_DIVERGENCE

Step D — Select exactly one next action:

    RESUME_CODER
    RESUME_REVIEWER
    RESUME_INTERNAL_FIX
    RESUME_TESTER_ATTEMPT
    RUN_REQUIRED_VERIFICATION
    COMPLETE_RUN
    WAIT_EXPLICIT_NEXT_TASK_START
    BLOCKED_NEEDS_HUMAN

The runtime should return the action. The model should not invent it.

## 14. Required invariants

1. No reset on resume. Workflow begin functions may only initialize absent/new state.
2. No source adoption without classification.
3. Run identity is immutable.
4. Input target and result target are distinct.
5. Canonical gate evidence is runtime-maintained.
6. Required UNVERIFIED evidence prevents terminal PASS.
7. Restart cannot duplicate a non-idempotent action.
8. Restart cannot silently skip a Reviewer/Tester/Human gate.
9. Handoff does not automatically activate the next phase.
10. Conversation history is never required to locate the next execution step.

## 15. Online vs Local gap map

| Capability | GSA Online | GSA Local | Required convergence |
|---|---|---|---|
| Project lifecycle resume | Present | Local process lifecycle | Keep separate from work resume |
| Run creation/completion | Present | Workflow-specific state | Keep exact attributed lineage |
| Interrupted Run resume | Missing first-class primitive | Partial state exists, entry resets | Add unified resume resolver |
| Exact source binding | Present but manually rewired during mutation | change-set + graph binding | Split input/result target |
| Checkpoint/state persistence | Project Memory records | SQLite latest_checkpoint/state | Keep transactional canonical pointer |
| Execution lease | No local-process lease equivalent | Present | Different mechanics, same single-owner invariant |
| Canonical verification pointer | Refs assembled by Run writer | Registry-specific evidence | Add current gate-evidence pointer |
| Recovery of source-ahead state | Manual reasoning/recovery child Run | Not first-class | Add reconciliation classification |
| Planning resume | New Run/revision decisions manual | begin resets state | Add planning entry resolver |
| Coding resume | New/recovery Runs manual | begin resets code state | Add code entry resolver |
| Explicit next-phase activation | Not fully separated from handoff | Controller can activate work | Add activation policy/boundary gate |

## 16. Proposed implementation order

1. Define shared ResumeDecision / RecoveryClassification contract.
2. Online: add open-Run resolver and interrupted-Run recovery primitive.
3. Online: separate Run input_target/result_target and canonical gate evidence.
4. Local: add resolve_planning_entry(); stop unconditional planning reset.
5. Local: add resolve_code_entry(); stop unconditional coding reset.
6. Local: reconstruct exact next agent packet from Registry after process restart.
7. Add source-vs-durable reconciliation and adoption record.
8. Add explicit handoff activation policy.
9. Add gate rule: required UNVERIFIED/BLOCKED verification cannot terminal-PASS.
10. Add structured evidence supersession/invalidation.
11. Add restart tests at every persisted workflow stage.
12. Add negative tests for source-ahead, stale Run, wrong target, duplicate attempt, orphan verification and stale lease recovery.

## 17. Resume test matrix

Automated tests should terminate/restart at each boundary and prove the exact next action:
- Planner before first revision;
- Planner after revision before Reviewer;
- Reviewer pending;
- Reviewer CHANGES_REQUIRED;
- Local CR pending;
- Internal Fix pending;
- Plan approved before graph registration;
- Graph registered before Job Pack activation;
- Coder before first mutation;
- Coder after mutation before checkpoint;
- Reviewer pending on exact change set;
- Reviewer CHANGES_REQUIRED;
- Internal Fix after one mutation;
- Reviewer PASS before deterministic verification;
- Verification BLOCKED;
- Verification PASS;
- Tester DUE;
- Tester RUNNING before executable step;
- Tester PREPARED fence;
- Tester report pending;
- Tester BLOCKED / NEEDS_HUMAN;
- Tester SATISFIED;
- Handoff available but next task not explicitly activated.

For every case assert:
- same exact Job/Task/JobPack;
- same or explicitly superseded Run lineage;
- no reset of completed checklist/evidence;
- no duplicate non-idempotent action;
- no skipped gate;
- correct exact source target;
- deterministic next role/action.

## Conclusion

The current architecture already has many correct primitives: exact revisions, stale-write rejection, checkpoints, leases, evidence records, graph binding and bounded gates.

The missing layer is a single recovery/resume protocol above those primitives.

Without that layer, an interrupted session can force the model to manually reconstruct and repair identity, target and evidence wiring. That is precisely the class of failure the runtime should own.

Full resume should mean:

    restart
      -> resolve durable state
      -> observe live source
      -> reconcile
      -> return one exact next action
      -> continue without reset, duplication, skipped gate or model guess
