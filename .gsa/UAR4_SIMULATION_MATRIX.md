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
