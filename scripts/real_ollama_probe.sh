#!/usr/bin/env bash
set -euo pipefail

# Run on a Mac with local Ollama. Supply GSA_UAR2B_MODEL=<installed-model>
# to select explicitly for this probe without changing application config.
# If omitted, this checks configured per-agent/default models and fails closed.
# A successful cargo process with zero matching tests is NOT a pass.
probe="workflow::tests::real_ollama_generated_terminal_schema_probe"
inventory="$(cargo test --lib -- --ignored --list 2>/dev/null)"
if ! grep -Fxq "$probe: test" <<< "$inventory"; then
  echo "UAR2B_PROBE_MISSING_ON_THIS_CHECKOUT: update source before trying the probe." >&2
  exit 1
fi

log="$(mktemp)"
trap 'rm -f "$log"' EXIT
# --show-output captures all three probe stages under a named test result,
# instead of interleaving println with libtest's "test ... ok" status.
if ! cargo test --lib "$probe" -- --ignored --exact --show-output 2>&1 | tee "$log"; then
  echo "UAR2B_PROBE_FAILED: inspect model/tool errors above." >&2
  exit 1
fi
bash scripts/verify_test_execution.sh "$log"
