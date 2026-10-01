# GSA Local Lessons Learned

## Purpose

This directory is the durable intake and patch-batching surface for workflow defects discovered while GSA Local is used on real projects.

The subsystem exists because synthetic/unit tests can prove local contracts, but they cannot prove that the workflow behaves correctly across real project state, long-running execution, Human gates, interruptions, external tools, Reviewer/Tester loops, and recovery.

Lessons collection is **non-blocking** for the active product Job.

## Authority

- Live project execution and durable evidence are the source of a lesson.
- The Lessons Collector may read project/runtime evidence and write only inside `lessons/`.
- The Lessons Collector must not repair product/runtime source.
- A lesson is not a patch request until it becomes CONFIRMED and enters a patch batch.
- Existing `WORKFLOW_LESSONS_LEARNED.md` is the legacy pre-subsystem corpus. New lessons belong here as individual entries.

## Directory layout

```text
lessons/
  README.md
  COLLECTOR_AGENT.md
  PATCH_POLICY.md
  entries/
    README.md
  batches/
    README.md
```

## Lesson lifecycle

```text
OBSERVED
  -> CONFIRMED
  -> BATCHED
  -> PATCHED
  -> VERIFIED
```

Alternative terminal states:

```text
OBSERVED -> DUPLICATE
OBSERVED -> NOT_WORKFLOW_DEFECT
CONFIRMED -> DEFERRED
```

A repeated manifestation of an existing defect is appended as evidence to the existing lesson and does not increase the patch threshold count.

## Intake rule

Prefer lessons discovered in real project execution. A synthetic/local test may support reproduction, but synthetic evidence alone should not be promoted to CONFIRMED when the claimed failure concerns real-project orchestration.

Each lesson should include:

- stable lesson id;
- date first observed;
- project/job/run/checkpoint context;
- classification;
- severity;
- exact observed behavior;
- expected workflow behavior;
- durable evidence references;
- reproducibility notes;
- impact;
- deduplication key;
- status;
- patch-batch reference when assigned.

## Patch threshold

Default threshold:

```text
5 unique CONFIRMED non-critical workflow defects
=> open one Patch Batch
```

CRITICAL defects may open a patch batch immediately.

The threshold counts defects, not occurrences. Multiple projects reproducing the same defect strengthen confidence but still count as one defect.

See `PATCH_POLICY.md`.
