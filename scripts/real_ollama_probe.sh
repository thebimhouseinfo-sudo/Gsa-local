#!/usr/bin/env bash
set -euo pipefail

# Run this on the actual Mac with Ollama and configured Reviewer/JobBuilder/Tester
# models. A successful cargo process with zero matching tests is NOT a pass.
probe="workflow::tests::real_ollama_generated_terminal_schema_probe"
inventory="$(cargo test --lib -- --ignored --list 2>/dev/null)"
if ! printf '%s\n' "$inventory" | grep -Fxq "$probe: test"; then
  echo "UAR2B_PROBE_MISSING_ON_THIS_CHECKOUT: update source before trying the probe." >&2
  exit 1
fi

log="$(mktemp)"
trap 'rm -f "$log"' EXIT
if ! cargo test --lib "$probe" -- --ignored --exact --nocapture 2>&1 | tee "$log"; then
  echo "UAR2B_PROBE_FAILED: inspect model/tool errors above." >&2
  exit 1
fi
bash scripts/verify_test_execution.sh "$log"
