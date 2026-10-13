# UAR-4 — guarded Mac QA recovery gate

Date: 2026-10-13. State: PENDING HUMAN. Do not claim UAR-4 PASS from synthetic CI.

## Verified evidence
- Human QA: isolated /Users/nam/Gsa-local-UAR3-QA.b5R8Pa, graph v1, M1/J1, code status CODER attempts=0; no persisted code checkpoint. README.md has source changes from old Coder run and the model returned invalid T1#4; T1 has only 1..3.
- Human preserved README.md and SQLite in /Users/nam/Gsa-local-UAR4-recovery-backup.
- GitHub canonical main source has no README.md at repository root; QA README needs explicit legacy-created-file adoption. Do not assume all unknown prior QA modifications are captured.
- The new recovery ledger supports explicit manually verified import, but neither a new model run nor the old source mutation are accepted evidence without an exact SHA and safe binding.

## Requirements before import
1. Stop the old QA GSA process via Ctrl-D at its own prompt. No force termination and no concurrent lease.
2. Update ~/Gsa-local from main. Verify canonical source still has no README.md.
3. Ensure README matches the human-provided backup; ensure SQLite code_workflow_state is exactly graph_version=1, jobpack_id=J1, status=CODER. Ensure no current ledger exists. Create a new consistent SQLite .backup before changing anything.
4. Back up QA src/workflow.rs and src/tools.rs, then copy these two source files from tested main.
5. **Only with Human acceptance of treating README as a Coder-created file**, write a bound ledger with graph_version 1, jobpack J1, path README.md, before_sha256 null, after_sha256 SHA256 of QA README. Do not change existing SQLite.
6. Resume with qwen38t:latest (Coder), independently selected installed Reviewer model, and existing approved Job Pack. Coder must inspect README and real J1 task positions, then submit its checkpoint without unnecessary rewrites.
7. Collect exact terminal output, file hashes and source commit, review verdict, and any Internal Fix results. If stale ledger or unexpected file mutation is detected, stop and escalate: do not overwrite/rebuild the SQLite registry.

## Boundaries
- Earlier CI simulations prove SQLite transitions and journal recovery mechanics; not live Ollama tool retry or independent CR.
- Do not run this migration on ~/Gsa-local/.gsa/state/gsa.db.
- The backup may have been taken while GSA was active; use sqlite3 .backup after graceful shutdown for a consistent recovery copy.
- No reset/abandon, no source overwrite of README, and no blind second Coder run without a verified ledger.

## Guarded executable (after user confirms and stops the old GSA)
The project includes `scripts/uar4-legacy-qa-recovery.sh`. After `git pull --ff-only origin main`, run:
```bash
bash scripts/uar4-legacy-qa-recovery.sh
```
This checks exact v1/J1/CODER SQLite state, README equality with the human backup, the absence of a canonical README on main, the lack of a competing GSA process/ledger, backs up QA Rust source and creates a consistent SQLite snapshot via `.backup`. It then copies the current tested `src/workflow.rs` and `src/tools.rs`, imports README's exact hash as a created-file recovery record, and launches GSA. The script never resets SQLite or overwrites README. It fails closed on mismatches and is not safe to re-run after successful ledger creation.
The script's Bash syntax is a mandatory GitHub CI check. This does NOT replace actual human approval of treating the old README as a Coder-created file.
