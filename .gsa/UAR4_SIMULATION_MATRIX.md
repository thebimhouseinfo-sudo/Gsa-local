# UAR-4 simulation matrix

Source checkpoint: main@927667e with code checkpoint from main@9c55f37.

Case A: model submits T1#4; Registry rejects an invalid claim and should leave status CODER.
Case B: corrected T1#1; expected checkpoint -> REVIEWER.
Case C: REVIEWER REVISE -> INTERNAL_FIX -> REVIEWER PASS.
Case D: process restarts after REVISE and resumes the same bound change set.
Case E: malformed, repeated, or mixed terminal submissions; retry bounded.
Case F: reviewer attempts project_write; denial and no mutation.

## First executable simulation result (2026-10-13)
- Exact simulation code on main@518489f7ac7b35f341672a7324fce4929be3f157.
- CI SUCCESS: https://github.com/thebimhouseinfo-sudo/Gsa-local/actions/runs/38381543317
- `uar4_simulate_model_invented_t1_position_four`: Registry rejects T1#4, retains CODER, writes no checkpoint.
- `uar4_simulate_resume_review_and_internal_fix`: real isolated SQLite transitions CODER -> REVIEWER -> INTERNAL_FIX -> REVIEW_PASS across Registry reopen.
- Scope boundary: deterministic Registry simulation, NOT an Ollama tool-calling or end-to-end model simulation. Model retry-on-invalid-claim and post-crash mutation provenance are not yet verified.
- Finding: the code loop accepts syntactically valid terminal arguments without validating claims against supplied active tasks; Registry correctly rejects invalid claim later. A bounded correction response in the same Coder role loop is a candidate repair, but must be validated by a model-response simulation before production patch.
- Source implementation remains unchanged by this simulation checkpoint. Do not mark UAR-4 PASS.

## Scoped claim retry implementation checkpoint (2026-10-13)
- Simulation precondition: exact SQLite/Registry simulation passed on main@518489f (CI 38381543317); invalid T1#4 fails closed and valid review/repair restart path passes.
- Scope validator `validate_code_claims` adds a pre-durable checkpoint gate against the active Job Pack's real checklist positions; duplicates/foreign positions fail closed.
- Shared coding loop now returns an error tool response to the Coder/Internal Fix model for claim mismatch, allowing a bounded resubmission in the same role session. The original Registry gate remains mandatory.
- Latest implementation code checkpoint: main@d1d3bb40d8ba2c8005056059638251e0a25f730f. CI verification pending; do not claim PASS before exact-head CI and real Ollama trial.
- Unresolved: durable mutation journal recovery after process interruption before checkpoint persistence. Model-response simulated retry must be distinguished from real model acceptance. Preserve original QA SQLite.
