# Lessons Learned Patch Policy

## Goal

Convert accumulated real-project workflow defects into bounded, reviewable maintenance releases instead of continuously modifying workflow architecture in the middle of unrelated product Jobs.

## Eligibility

A lesson counts toward the normal patch threshold only when all are true:

- status = `CONFIRMED`;
- it represents a unique defect mechanism;
- durable evidence exists;
- it is not already assigned to another open patch batch;
- it is not merely a product-specific implementation bug.

## Threshold

Normal patch batch trigger:

```text
5 eligible CONFIRMED unique defects
```

Critical override:

```text
>= 1 eligible CRITICAL defect
=> patch batch may open immediately
```

Repeated occurrences of the same dedupe key do not increase the count.

## Patch batch

A patch batch is a separate bounded GSA Job.

It must not be silently merged into the active product Job that discovered the lessons.

Each batch should:

1. freeze its included lesson IDs;
2. reconstruct exact evidence for each lesson;
3. identify shared root causes;
4. produce a bounded implementation plan;
5. run normal Planner -> Reviewer -> automatic Local CR planning finalization;
6. implement fixes through Coder/self-check/Reviewer loops;
7. add regression coverage for each fixed mechanism;
8. test the patch on at least one representative real project when the defect is real-project dependent;
9. publish/release only after patch verification.

## Batch sizing

Five confirmed defects is a trigger, not a requirement to force all five into one source change.

Planner may split them into multiple linked patch Jobs when their causal chains are materially different, while keeping them under the same patch batch/release train.

## Release rule

A patch may be called complete only when:

- every included lesson is either PATCHED or explicitly deferred with reason;
- relevant deterministic regression passes;
- real-project-dependent fixes have real-project verification;
- no patch introduces a new blocking workflow regression.

After release, patched lessons move to `VERIFIED` only after their corrected behavior is observed on a real project or equivalent authoritative environment.

## Non-goal

This policy is not an excuse to postpone a CRITICAL correctness or safety defect until five issues accumulate.
