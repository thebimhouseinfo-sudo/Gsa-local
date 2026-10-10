#!/usr/bin/env bash
set -euo pipefail

# Run cheap script syntax and adversarial verifier controls before compiling
# Rust. A broken guard is a failing CI, not an optional post-test warning.
bash -n scripts/ci.sh scripts/real_ollama_probe.sh scripts/verify_test_execution.sh
bash scripts/verify_test_execution.sh --self-test

cargo fmt --check
cargo check
cargo test

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
  if ! grep -Fxq "$test_name: test" <<< "$all_tests"; then
    echo "CI_REQUIRED_TEST_MISSING=$test_name" >&2
    exit 1
  fi
done
if [[ "$(uname -s)" == "Darwin" ]]; then
  for test_name in \
    macos_production_runner_cannot_falsely_pass_missing_executable \
    macos_production_runner_must_execute_real_successfully_sandboxed_command; do
    if ! grep -Fxq "$test_name: test" <<< "$all_tests"; then
      echo "CI_REQUIRED_MAC_REAL_RUNNER_CONTROL_MISSING=$test_name" >&2
      exit 1
    fi
  done
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
