#!/usr/bin/env bash
# Read-only audit of an existing project's GSA SQLite database.
# The Rust registry migration runs exclusively against a temporary backup.
set -euo pipefail

if [[ $# -ne 1 || ! -f "$1" ]]; then
  echo "usage: scripts/uar3-local-db-audit.sh /absolute/path/to/.gsa/state/gsa.db" >&2
  exit 2
fi
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

python3 - "$1" "$tmp/snapshot.db" <<'PY'
import sqlite3
import sys
from pathlib import Path

original = Path(sys.argv[1]).expanduser().resolve(strict=True)
snapshot = Path(sys.argv[2])
readonly = sqlite3.connect(original.as_uri() + "?mode=ro", uri=True)
copy = sqlite3.connect(str(snapshot))
try:
    readonly.backup(copy)
    copy.commit()
    print("UAR3_SNAPSHOT_CREATED=1")
    for table in ("plan_revisions", "plan_workflow_state", "approved_plan", "execution_graph"):
        exists = copy.execute(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?", (table,)
        ).fetchone()[0]
        if exists:
            count = copy.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
            print(f"UAR3_BEFORE_{table.upper()}={count}")
finally:
    copy.close()
    readonly.close()
PY

(
  cd "$repo_root"
  cargo run --quiet --bin uar3-db-probe -- "$tmp/snapshot.db"
)

python3 - "$tmp/snapshot.db" <<'PY'
import sqlite3
import sys
db = sqlite3.connect(sys.argv[1])
try:
    integrity = db.execute("PRAGMA integrity_check").fetchone()[0]
    print("UAR3_SNAPSHOT_INTEGRITY=" + str(integrity))
    if integrity != "ok":
        raise SystemExit(1)
finally:
    db.close()
PY
echo "UAR3_ORIGINAL_DB_UNMODIFIED=1 (read-only connection; migration ran on snapshot)"
