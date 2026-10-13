# UAR-4 simulation matrix

Source checkpoint: main@927667e with code checkpoint from main@9c55f37.

Case A: model submits T1#4; Registry rejects an invalid claim and should leave status CODER.
Case B: corrected T1#1; expected checkpoint -> REVIEWER.
Case C: REVIEWER REVISE -> INTERNAL_FIX -> REVIEWER PASS.
Case D: process restarts after REVISE and resumes the same bound change set.
Case E: malformed, repeated, or mixed terminal submissions; retry bounded.
Case F: reviewer attempts project_write; denial and no mutation.
