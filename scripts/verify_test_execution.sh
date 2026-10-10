#!/usr/bin/env bash
set -euo pipefail

probe="workflow::tests::real_ollama_generated_terminal_schema_probe"

count_exact() {
  grep -Fxc -- "$2" "$1" || true
}

count_pattern() {
  grep -Ec "$2" "$1" || true
}

verify_result() {
  local output="$1"
  # Cargo can exit 0 with 0 selected tests. Require the libtest --show-output
  # capture header for this exact test, plus all three real-model stages.
  local summary='^test result: ok[.] 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9]+([.][0-9]+)?s$'
  local verdict='^UAR2B_PROBE verdict model=[^[:space:]]+ elapsed_ms=[0-9]+ repairs=1 invocations=[0-9]+$'
  local job_builder='^UAR2B_PROBE job_builder model=[^[:space:]]+ elapsed_ms=[0-9]+ repairs=[0-9]+ invocations=[0-9]+$'
  local tester='^UAR2B_PROBE tester model=[^[:space:]]+ elapsed_ms=[0-9]+ repairs=[0-9]+ invocations=[0-9]+$'
  local capture
  capture="$(awk -v header="---- ${probe} stdout ----" '
    $0 == header { inside=1; next }
    inside && /^test result:/ { exit }
    inside { print }
  ' "$output")"

  if [[ "$(count_exact "$output" 'running 1 test')" != "1" ]] ||
     [[ "$(count_exact "$output" "test ${probe} ... ok")" != "1" ]] ||
     [[ "$(count_exact "$output" "---- ${probe} stdout ----")" != "1" ]] ||
     [[ "$(count_pattern "$output" '^test result:')" != "1" ]] ||
     [[ "$(count_pattern "$output" "$summary")" != "1" ]] ||
     [[ "$(grep -Ec "$verdict" <<< "$capture" || true)" != "1" ]] ||
     [[ "$(grep -Ec "$job_builder" <<< "$capture" || true)" != "1" ]] ||
     [[ "$(grep -Ec "$tester" <<< "$capture" || true)" != "1" ]] ||
     grep -Eq '^test [^[:space:]]+ [.]\.\. (FAILED|ignored)$|^error: test failed|^test result: FAILED[.]' "$output"; then
    echo "UAR2B_PROBE_NOT_VERIFIED: require exactly one executed PASS and verdict/JobBuilder/Tester model evidence; reject zero tests and mixed/failed suites." >&2
    return 1
  fi
  echo "UAR2B_PROBE_EXECUTED_PASS=$probe"
}

if [[ "${1:-}" == "--self-test" ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  printf '%s\n' \
    'running 1 test' \
    "test ${probe} ... ok" \
    'successes:' \
    "---- ${probe} stdout ----" \
    'UAR2B_PROBE verdict model=qwen38t:latest elapsed_ms=123 repairs=1 invocations=2' \
    'UAR2B_PROBE job_builder model=qwen38t:latest elapsed_ms=456 repairs=0 invocations=1' \
    'UAR2B_PROBE tester model=qwen38t:latest elapsed_ms=789 repairs=0 invocations=1' \
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.10s' > "$tmp/good"
  printf '%s\n' \
    'running 0 tests' \
    'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.00s' > "$tmp/zero"
  sed 's/ ... ok/ ... FAILED/; s/result: ok. 1 passed; 0 failed/result: FAILED. 0 passed; 1 failed/' "$tmp/good" > "$tmp/failed"
  sed "s/test ${probe} ... ok/test other_test ... ok/" "$tmp/good" > "$tmp/wrong"
  grep -v '^UAR2B_PROBE tester ' "$tmp/good" > "$tmp/missing_stage"
  { grep '^UAR2B_PROBE ' "$tmp/good"; grep -v '^UAR2B_PROBE ' "$tmp/good"; } > "$tmp/outside_capture"
  sed '/^UAR2B_PROBE verdict /p' "$tmp/good" > "$tmp/duplicate_stage"
  grep -v -- "^---- ${probe} stdout ----$" "$tmp/good" > "$tmp/missing_capture"
  sed 's/repairs=1/repairs=0/' "$tmp/good" > "$tmp/no_repair"
  cat "$tmp/good" "$tmp/good" > "$tmp/duplicate_suites"
  { cat "$tmp/good"; echo 'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s'; } > "$tmp/mixed_failure"

  verify_result "$tmp/good" > /dev/null
  for case in zero failed wrong missing_stage missing_capture outside_capture duplicate_stage no_repair duplicate_suites mixed_failure; do
    if verify_result "$tmp/$case" > /dev/null 2>&1; then
      echo "TEST_GUARD_FALSE_GREEN: $case should not pass" >&2
      exit 1
    fi
  done
  echo "TEST_GUARD_NEGATIVE_CONTROLS_PASS=zero,failed,wrong,missing_stage,missing_capture,outside_capture,duplicate_stage,no_repair,duplicate_suites,mixed_failure"
  exit 0
fi

if [[ "$#" -ne 1 ]] || [[ ! -f "$1" ]]; then
  echo "Usage: $0 <captured-cargo-test-output> | --self-test" >&2
  exit 2
fi

verify_result "$1"
