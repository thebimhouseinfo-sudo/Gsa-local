# Stock-QA S4 — Human Mac Acceptance Gate

> **Status:** PREPARED, NOT EXECUTED. Execute only after exact-main CI SUCCESS and independent code review of S1–S3. This document is not an approval to mutate Stock-QA data.
>
> **Goal:** repair only the unreachable `chk-j1-verify` boundary on the retained real Stock-QA graph, keep J1 source/review/verification/Local CR unchanged, and prove `J1 DONE -> Tester OBSERVED -> J2 ACTIVE`.

## Source and no-replay baseline

- Canonical source: `https://github.com/thebimhouseinfo-sudo/Gsa-local` on `main`; record exact HEAD and matching GitHub Actions run.
- Real project: `~/Projects/GSA-Stock-QA`. Do **not** reset `.gsa/state/gsa.db`, rewrite old evidence, re-register a graph, or rerun J1 Coder.
- Known historical J1 change set: `a7dabdfaa88c700e0f270c8f327b43d913535ce0bd9588cdde62117a56b65753`. Check actual readback before any mutation; fail closed if different.
- Human must stop any GSA process holding the project lease normally before backups or update. Avoid competing GSA processes.

## Before Human approval

1. Verify the final `main` HEAD and exact successful CI/checkpoint code review. Update source with `git pull --ff-only origin main` in `~/Gsa-local`, then build `cargo build --release`.
2. From the isolated Stock-QA project, make a **consistent SQLite backup** with SQLite `.backup` after all GSA processes exit. Preserve project source and logs separately; do not delete WAL/SHM or edit SQLite tables manually. Check DB integrity and the current graph/J1/J2/checkpoint state read-only.
3. Start the resulting canonical GSA binary **with working directory `~/Projects/GSA-Stock-QA`**. Choose distinct configured Reviewer and Local CR models. Confirm the original Human requirement is available in durable planning state.
4. At the GSA prompt enter `GRAPH REPAIR`. Verify the displayed exact checkpoint diff `MILESTONE_GATE -> AFTER_JOBPACK_SET`, original plan and graph binding, proposal hash, J1 change set, and unchanged required `ev-j1-verify` for J2. If any differ, stop.
5. GSA must record independent Reviewer and Local CR PASS for the exact proposal. Human then types the **exact approval text that GSA displays**: `APPROVE GRAPH REPAIR <actual proposal hash>`. Do not enter approval before reviewing the proposal and review results.

## After successful apply

1. Read back `graph_version=1`, checkpoint `chk-j1-verify` now `AFTER_JOBPACK_SET`, monotonic contract revision `1`, immutable baseline revision `0`, and exactly one `GRAPH_CONTRACT_AMENDED` event.
2. Confirm J1 source SHA, `change_set_id`, prior Code Review, verification result and Local CR are identical and J2 has not activated prematurely.
3. Resume ordinary GSA execution. J1 must terminalize as `DONE`; Tester checkpoint must become due and run independent `PROJECT_VERIFICATION`, including actual FEFO-ordering and insufficient-stock atomic rollback negative cases. Only a truly observed matching output `ev-j1-verify` may release J2.
4. Observe `J1 DONE -> Tester OBSERVED -> J2 ACTIVE` on the same graph version. Stop and report exact error/status if Tester yields FAIL, NEEDS_HUMAN, unsupported capability, stale evidence or conflict; never fabricate PASS.
5. Run J2 through its ordinary coding/review/verification/CR gates, then run the independent external oracle (if the original fixture kit exists): `bash ~/Downloads/gsa-stock-qa-kit/check_acceptance.sh ~/Projects/GSA-Stock-QA`. The oracle is not a substitute for Tester evidence.
6. Test a **normal process restart** and read back graph contract revision/event count to check replay idempotence; do not issue a second conflicting repair.
7. Preserve console transcript, exact HEAD, original/after J1 source hashes, database read-only before/after summary, Tester attempt/output IDs and oracle results. Mark HUMAN_TEST_REQUIRED until complete.

## Fail-closed outcomes

- Original approved PlanArtifact or Human requirement contradicts timing change: do not approve; requires separately reviewed replan.
- Existing Tester attempt/output, missing exact J1 source, J2 already started, wrong lease/workspace, projection mismatch, changed CR authority fingerprint or missing role review: `GRAPH_REPAIR_BLOCKED`, no partial write.
- A new CI SUCCESS alone is **not** proof of real Mac acceptance. Do not mark S4 PASS without recorded Human evidence.


## Latest real Mac S4 finding — 2026-10-15

**BLOCKED at Reviewer format validation; NOT S4 PASS.** The exact graph proposal `05ac70c4988c0171491b8b2a2606cdf34e6ad38a39ab2d5ed91756eac239431d` remained staged. Real Reviewer `qwen38t:latest` began, then exited with `GRAPH_REPAIR_BLOCKED: reviewer failed strict JSON contract: expected value at line 1 column 1`. This is a format parsing error, not a FAIL verdict. Local CR `gemma4:31b-mlx`, Human approval, amendment, Tester OBSERVED and J2 ACTIVE have not occurred.

Both agent models are correctly configured at `~/Library/Application Support/gsa/config.toml`; `GSA_WORKFLOW_MODEL` is unset. Fix the CLI reviewer structured-response contract and bounded error recovery; CI and independent CR before resuming real Mac test. Preserve the existing graph cursor and J1 evidence. See `.gsa/PROJECT_STATUS.md` current handoff for full technical instructions.
