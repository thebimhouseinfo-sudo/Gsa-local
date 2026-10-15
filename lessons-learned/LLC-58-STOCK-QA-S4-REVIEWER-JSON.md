# LLC-58 — Stock-QA S4 Reviewer JSON parsing failure

Date: 2026-10-15. Status: COLLECTED, UNRESOLVED. Real Mac project GSA-Stock-QA.

Observed: Graph repair preview succeeded and preserved the original J1 change set. Correct macOS model configuration enabled Reviewer qwen38t:latest to start, but its output failed strict JSON parsing with "expected value at line 1 column 1". The raw response has not been captured; do not assume whether it was empty, prose or markdown. There was no Reviewer PASS, Local CR review, human approval or graph amendment.

Root cause scope: src/app.rs graph-repair review path expects a single JSON object directly from a plain streamed Ollama response without bounded formatting repair or useful sanitized parse diagnostics. This was not caught by registry/controller synthetic tests.

Next action: inspect existing structured-output support in src/ollama.rs and shared agent runtime; implement strict structured response/validated format-only retries with bounded failure; test valid PASS and FAIL, malformed, empty, fenced and prose responses, preserving no-false-PASS and durable pending cursor; CI, independent code CR, then retest existing proposal on real Mac. Do not replay J1 or manually modify the database.

Configuration lesson: on macOS AppConfig::config_path via dirs::config_dir is ~/Library/Application Support/gsa/config.toml; GSA_WORKFLOW_MODEL overrides agent-specific model selection. Full exact hashes and handoff are in .gsa/PROJECT_STATUS.md.
