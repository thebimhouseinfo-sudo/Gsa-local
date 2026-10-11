#!/usr/bin/env bash
# Launch a real-model UAR-3 restart test against an isolated project and a
# standalone copy of the user's GSA database. Never alter the original DB.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
db="$root/.gsa/state/gsa.db"
if [[ ! -f "$db" ]]; then
  echo "UAR3_QA_BLOCKED=ORIGINAL_DB_MISSING $db" >&2
  exit 2
fi
if ! command -v lsof >/dev/null 2>&1; then
  echo "UAR3_QA_BLOCKED=LSOF_UNAVAILABLE" >&2
  exit 2
fi
for suffix in -wal -shm -journal; do
  if [[ -e "${db}${suffix}" ]]; then
    echo "UAR3_QA_BLOCKED=SQLITE_SIDECAR_PRESENT $suffix" >&2
    exit 2
  fi
done
set +e
lsof -t "$db" >/dev/null 2>&1
handles_rc=$?
set -e
if [[ "$handles_rc" -ne 1 ]]; then
  echo "UAR3_QA_BLOCKED=DB_OPEN_OR_LSOF_ERROR rc=$handles_rc" >&2
  exit 2
fi

qa_model="${GSA_WORKFLOW_MODEL:-qwen38t:latest}"
if ! command -v ollama >/dev/null 2>&1; then
  echo "UAR3_QA_BLOCKED=OLLAMA_CLI_NOT_FOUND" >&2
  exit 2
fi
if ! ollama list | awk 'NR>1 {print $1}' | grep -Fxq "$qa_model"; then
  echo "UAR3_QA_BLOCKED=WORKFLOW_MODEL_NOT_INSTALLED model=$qa_model" >&2
  exit 2
fi
export GSA_WORKFLOW_MODEL="$qa_model"
qa="$(mktemp -d "$HOME/Gsa-local-UAR3-QA.XXXXXX")"
rsync -a --exclude='.git/' --exclude='.gsa/' --exclude='target/' \
  --exclude='.env' --exclude='.env.*' "$root/" "$qa/"
mkdir -p "$qa/.gsa/state"
before="$(stat -f '%z:%m:%i' "$db")"
cp "$db" "$qa/.gsa/state/gsa.db"
after="$(stat -f '%z:%m:%i' "$db")"
if [[ "$before" != "$after" ]]; then
  echo "UAR3_QA_BLOCKED=ORIGINAL_DB_CHANGED_DURING_COPY" >&2
  exit 2
fi
for suffix in -wal -shm -journal; do
  if [[ -e "${db}${suffix}" ]]; then
    echo "UAR3_QA_BLOCKED=SQLITE_SIDECAR_APPEARED $suffix" >&2
    exit 2
  fi
done

echo "UAR3_QA_ISOLATED_PROJECT=$qa"
echo "UAR3_QA_WORKFLOW_MODEL=$qa_model"
echo "UAR3_QA_ORIGINAL_DB_UNTOUCHED=1"
echo "UAR3_QA_INSTRUCTIONS=Ask Planner to start a SMALL documentation-only Implementation Plan. The legacy recovery confirmation is safe ONLY in this QA copy."
echo "UAR3_QA_INSTRUCTIONS=When PLAN_STAGE Reviewer START appears, press Ctrl+C to simulate process interruption."
echo "UAR3_QA_RESTART_COMMAND=cd '$qa' && GSA_WORKFLOW_MODEL='$qa_model' cargo run --bin gsa"
echo "UAR3_QA_INSTRUCTIONS=After restart, type RESUME at the saved-planning prompt. Report the full output."
cd "$qa"
cargo run --bin gsa
