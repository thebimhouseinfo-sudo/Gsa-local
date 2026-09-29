# GSA Local — Detailed Execution Plan

This plan turns `IMPLEMENTATION_PLAN.md` into bounded implementation Jobs.

## Milestone A — Runtime foundation
Current Job `J-8E38`:
- T-BOOT — Rust crate, `gsa` binary, module layout, local CI.
- T-CONFIG — persistent agent→model presets and session-only hot swap.
- T-OLLAMA — native `/api/tags`, `/api/show`, streaming `/api/chat`.
- T-HARNESS — Shared GSA rules + localized role contracts.
- T-REGISTRY — SQLite plan binding, events, checkpoint and execution lease.
- T-WIRE — interactive shell and model dispatch.

Gate: `cargo fmt --check && cargo check && cargo test`, then code review.

## Milestone B — Planning workflow and Job Builder
Implement revisioned Implementation Plan, Planner↔Reviewer, automatic Local CR, Internal Fix, PLAN_APPROVED(revision/hash), Job Builder decomposition, and transactional execution-graph registration.

## Milestone C — Milestone / Job Pack controller
Implement Job Pack states, dependency resolution, Milestone lock/unlock, checkpoint resume, active Job Pack context, and no-jump enforcement.

## Milestone D — Coding / Verification workflow
Implement Coder↔Reviewer, Verification Controller, Local CI discovery, read-only Tester, automatic Local CR final gate, Job Pack completion and Milestone verification.

## Milestone E — Hardening
Cover path/symlink escape, shell redirection, external git targeting, crash recovery, stale lease, state corruption, fake completion, weak tests, and loop exhaustion.

Only the current Job is implemented at a time. Runtime state, not Markdown checkboxes, becomes authority when the relevant subsystem exists.
