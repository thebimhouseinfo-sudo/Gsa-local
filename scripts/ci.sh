#!/usr/bin/env bash
set -euo pipefail

cargo fmt --check
cargo check
cargo test

# CI must make environment-gated tests explicit. A green offline CI run must not
# silently imply that local-Ollama compatibility probes were executed.
expected_ignored_test="workflow::tests::real_ollama_generated_terminal_schema_probe"
ignored_tests="$(cargo test -- --ignored --list 2>/dev/null | sed -n 's/: test$//p')"

if [[ "$ignored_tests" != "$expected_ignored_test" ]]; then
  echo "Unexpected ignored-test inventory." >&2
  echo "Expected:" >&2
  echo "$expected_ignored_test" >&2
  echo "Observed:" >&2
  if [[ -n "$ignored_tests" ]]; then
    echo "$ignored_tests" >&2
  else
    echo "<none>" >&2
  fi
  exit 1
fi

echo "CI_EXTERNAL_TEST_NOT_RUN=$expected_ignored_test (requires local Ollama and configured role models)"
