#!/usr/bin/env bash
set -euo pipefail

SOURCE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
QA="${1:-/Users/nam/Gsa-local-UAR3-QA.b5R8Pa}"
BACKUP="${2:-$HOME/Gsa-local-UAR4-recovery-backup}"
STATE_DB="$QA/.gsa/state/gsa.db"
LEDGER="$QA/.gsa/state/code_write_recovery.json"

if pgrep -fl 'target/debug/gsa|cargo run.*gsa'; then
  echo "STOP: a GSA process is running. Exit the old QA GSA via Ctrl-D first." >&2
  exit 1
fi

test "$(git -C "$SOURCE" branch --show-current)" = "main"
test ! -e "$SOURCE/README.md"
test -f "$QA/README.md"
test -f "$BACKUP/README.md"
test -f "$BACKUP/gsa.db"
test -f "$STATE_DB"
test -f "$SOURCE/src/workflow.rs"
test -f "$SOURCE/src/tools.rs"
test -f "$QA/src/workflow.rs"
test -f "$QA/src/tools.rs"
test ! -e "$LEDGER"

cmp -s "$QA/README.md" "$BACKUP/README.md"
STATE="$(sqlite3 -readonly "$STATE_DB" "SELECT graph_version||':'||jobpack_id||':'||status FROM code_workflow_state WHERE id=1;")"
if [[ "$STATE" != "1:J1:CODER" ]]; then
  echo "STOP: unexpected QA workflow state: $STATE" >&2
  exit 1
fi

mkdir -p "$BACKUP"
SNAPSHOT="$(mktemp -d "$BACKUP/uar4-source.XXXXXX")"
cp -p "$QA/src/workflow.rs" "$SNAPSHOT/workflow.rs"
cp -p "$QA/src/tools.rs" "$SNAPSHOT/tools.rs"
sqlite3 "$STATE_DB" ".backup '$SNAPSHOT/gsa-consistent.db'"

README_HASH="$(shasum -a 256 "$QA/README.md" | awk '{print $1}')"
cp -p "$SOURCE/src/workflow.rs" "$QA/src/workflow.rs"
cp -p "$SOURCE/src/tools.rs" "$QA/src/tools.rs"

TMP="$LEDGER.import"
if [[ -e "$TMP" ]]; then
  echo "STOP: pending ledger import staging file already exists" >&2
  exit 1
fi
printf '{"graph_version":1,"jobpack_id":"J1","records":[{"path":"README.md","before_sha256":null,"after_sha256":"%s"}]}\n' "$README_HASH" > "$TMP"
ln "$TMP" "$LEDGER"
rm "$TMP"

echo "UAR4_RECOVERY_IMPORT_READY source=$(git -C "$SOURCE" rev-parse --short HEAD)"
echo "UAR4_RECOVERY_IMPORT_READY readme_sha256=$README_HASH"
echo "UAR4_RECOVERY_IMPORT_READY consistent_backup=$SNAPSHOT/gsa-consistent.db"
echo "NO SQLITE MUTATION PERFORMED"

cd "$QA"
GSA_WORKFLOW_MODEL='qwen38t:latest' cargo run --bin gsa
