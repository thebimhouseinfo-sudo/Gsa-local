#!/usr/bin/env bash
set -euo pipefail

cargo fmt --check
cargo check
cargo test

# Guard against the most dangerous false-green case: a filtered cargo test
# exits successfully even when no test with that name was executed.
bash scripts/verify_test_execution.sh --self-test

# Verify the real test inventory on this revision, not just a successful
# cargo exit status. These core tests bind the suite to actual workflow gates.
all_tests="$(cargo test -- --list 2>/dev/null)"
active_count="$(printf '%s\n' "$all_tests" | grep -Ec ': test$' || true)"
if (( active_count < 100 )); then
  echo "CI_TEST_INVENTORY_TOO_SMALL=$active_count (expected at least 100 tests)" >&2
  exit 1
fi
for test_name in \
  workflow::tests::real_ollama_generated_terminal_schema_probe \
  workflow::tests::terminal_schema_required_fields_follow_serde_defaults \
  workflow::tests::execution_graph_schema_is_typed_and_conditionals_are_runtime_validated \
  successful_build_only_evidence_is_not_a_test_pass \
  required_tester_evidence_fails_closed_when_missing; do
  if ! printf '%s\n' "$all_tests" | grep -Fxq "$test_name: test"; then
    echo "CI_REQUIRED_TEST_MISSING=$test_name" >&2
    exit 1
  fi
done
if [[ "$(uname -s)" == "Darwin" ]] && ! printf '%s\n' "$all_tests" | grep -Fxq 'macos_production_runner_cannot_falsely_pass_missing_executable: test'; then
  echo "CI_REQUIRED_MAC_NEGATIVE_CONTROL_MISSING" >&2
  exit 1
fi
echo "CI_TESTS_DISCOVERED=$active_count"

# The real-model probe needs installed Ollama models and therefore does not
# execute in offline CI. Its exclusion must be explicit and uniquely bounded.
expected_ignored_test="workflow::tests::real_ollama_generated_terminal_schema_probe"
ignored_tests="$(cargo test -- --ignored --list 2>/dev/null | sed -n 's/: test$//p')"
if [[ "$ignored_tests" != "$expected_ignored_test" ]]; then
  echo "Unexpected ignored-test inventory." >&2
  echo "Expected: $expected_ignored_test" >&2
  echo "Observed: ${ignored_tests:-<none>}" >&2
  exit 1
fi
echo "CI_EXTERNAL_TEST_NOT_RUN=$expected_ignored_test (requires local Ollama and configured role models)"
