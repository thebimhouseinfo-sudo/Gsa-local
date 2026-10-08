#!/usr/bin/env bash
set -euo pipefail

probe="workflow::tests::real_ollama_generated_terminal_schema_probe"

verify_result() {
  local output="$1"
  # An exit code of 0 alone is insufficient: cargo returns success when the
  # requested filter matches zero tests. Verify both the exact test and summary.
  local passed
  passed="$(grep -Fxc "test ${probe} ... ok" "$output" || true)"
  if [[ "$passed" != "1" ]] || ! grep -Eq '^test result: ok[.] 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in ' "$output"; then
    echo "UAR2B_PROBE_NOT_VERIFIED: expected exactly one executed PASS; zero filtered tests, other tests or failures cannot count." >&2
    return 1
  fi
  echo "UAR2B_PROBE_EXECUTED_PASS=$probe"
}

if [[ "${1:-}" == "--self-test" ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  printf 'running 1 test\ntest %s ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.10s\n' "$probe" > "$tmp/good"
  printf 'running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.00s\n' > "$tmp/zero"
  printf 'running 1 test\ntest %s ... FAILED\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.01s\n' "$probe" > "$tmp/failed"
  printf 'running 1 test\ntest other_test ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.01s\n' > "$tmp/wrong"
  verify_result "$tmp/good" > /dev/null
  for case in zero failed wrong; do
    if verify_result "$tmp/$case" > /dev/null 2>&1; then
      echo "TEST_GUARD_FALSE_GREEN: $case should not pass" >&2
      exit 1
    fi
  done
  echo "TEST_GUARD_NEGATIVE_CONTROLS_PASS=zero,failed,wrong"
  exit 0
fi

if [[ "$#" -ne 1 ]] || [[ ! -f "$1" ]]; then
  echo "Usage: $0 <captured-cargo-test-output> | --self-test" >&2
  exit 2
fi

verify_result "$1"
