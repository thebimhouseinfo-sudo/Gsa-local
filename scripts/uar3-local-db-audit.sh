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

python3 - "$1" "$tmp/snapshot.db" "$tmp/counts.json" <<'PY'
import json
import os
import shutil
import subprocess
import sqlite3
import sys
from pathlib import Path

original = Path(sys.argv[1]).expanduser().resolve(strict=True)
snapshot = Path(sys.argv[2])
readonly = None
used_copy_fallback = False
try:
    readonly = sqlite3.connect(original.as_uri() + "?mode=ro", uri=True)
    readonly.execute("PRAGMA schema_version").fetchone()
except (sqlite3.Error, OSError) as exc:
    print(f"UAR3_SOURCE_READ_FAILED={type(exc).__name__}: {exc}", file=sys.stderr)
    for candidate in (original.parent, original, Path(str(original) + "-wal"), Path(str(original) + "-shm")):
        try:
            st = candidate.stat()
            print(
                f"UAR3_PATH_DIAGNOSTIC={candidate.name} exists=1 "
                f"mode={oct(st.st_mode & 0o777)} size={st.st_size} "
                f"readable={os.access(candidate, os.R_OK)} "
                f"writable={os.access(candidate, os.W_OK)}",
                file=sys.stderr,
            )
        except OSError as detail:
            print(f"UAR3_PATH_DIAGNOSTIC={candidate.name} error={detail}", file=sys.stderr)
    try:
        with original.open("rb") as source:
            signature = source.read(16)
        header_valid = signature == bytes.fromhex("53514c69746520666f726d6174203300")
        print(f"UAR3_SQLITE_HEADER_VALID={int(header_valid)}", file=sys.stderr)
    except OSError as detail:
        print(f"UAR3_HEADER_READ_FAILED={detail}", file=sys.stderr)
    # Some SQLite WAL-mode databases cannot be opened with mode=ro when
    # sidecars are absent. Only copy the standalone database after a strict
    # no-sidecars/no-open-handles gate; never open the original writable.
    sidecars = [Path(str(original) + suffix) for suffix in ("-wal", "-shm", "-journal")]
    if any(item.exists() for item in sidecars):
        raise SystemExit("UAR3_COPY_FALLBACK_BLOCKED=SIDECAR_PRESENT")
    try:
        probe = subprocess.run(["lsof", "-t", str(original)], capture_output=True, text=True)
    except OSError as detail:
        raise SystemExit(f"UAR3_COPY_FALLBACK_BLOCKED=LSOF_UNAVAILABLE {detail}")
    if probe.returncode != 1:
        raise SystemExit(f"UAR3_COPY_FALLBACK_BLOCKED=FILE_OPEN_OR_LSOF_ERROR rc={probe.returncode}")
    before = original.stat()
    shutil.copyfile(original, snapshot)
    after = original.stat()
    if (before.st_size, before.st_mtime_ns, before.st_ino) != (
        after.st_size, after.st_mtime_ns, after.st_ino
    ) or any(item.exists() for item in sidecars):
        raise SystemExit("UAR3_COPY_FALLBACK_BLOCKED=SOURCE_CHANGED_DURING_COPY")
    probe_after = subprocess.run(["lsof", "-t", str(original)], capture_output=True, text=True)
    if probe_after.returncode != 1:
        raise SystemExit("UAR3_COPY_FALLBACK_BLOCKED=FILE_OPENED_DURING_COPY")
    used_copy_fallback = True
    print("UAR3_STANDALONE_COPY_FALLBACK=1")

try:
    copy = sqlite3.connect(str(snapshot))
    if not used_copy_fallback:
        readonly.backup(copy)
except sqlite3.Error as exc:
    raise SystemExit(
        f"UAR3_BACKUP_FAILED={type(exc).__name__}: {exc}; "
        f"source={original}; snapshot_dir={snapshot.parent}; "
        "check SQLite WAL/-shm permissions and temporary directory access"
    )
try:
    copy.commit()
    print("UAR3_SNAPSHOT_CREATED=1")
    counts = {}
    for table in ("plan_revisions", "plan_workflow_state", "approved_plan", "execution_graph"):
        exists = copy.execute(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?", (table,)
        ).fetchone()[0]
        if exists:
            count = copy.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
            counts[table] = count
            print(f"UAR3_BEFORE_{table.upper()}={count}")
    Path(sys.argv[3]).write_text(json.dumps(counts))
finally:
    copy.close()
    if readonly is not None:
        readonly.close()
PY

(
  cd "$repo_root"
  cargo run --quiet --bin uar3_db_probe -- "$tmp/snapshot.db"
)

python3 - "$tmp/snapshot.db" "$tmp/counts.json" <<'PY'
import json
import sqlite3
import sys
from pathlib import Path
db = sqlite3.connect(sys.argv[1])
try:
    integrity = db.execute("PRAGMA integrity_check").fetchone()[0]
    print("UAR3_SNAPSHOT_INTEGRITY=" + str(integrity))
    if integrity != "ok":
        raise SystemExit(1)
    for table, expected in json.loads(Path(sys.argv[2]).read_text()).items():
        actual = db.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
        print(f"UAR3_AFTER_{table.upper()}={actual}")
        if actual != expected:
            raise SystemExit(f"UAR3_DATA_LOSS_OR_DUPLICATION={table}")
    print("UAR3_LEGACY_ROW_COUNTS_PRESERVED=1")
finally:
    db.close()
PY
echo "UAR3_ORIGINAL_DB_UNMODIFIED=1 (source opened read-only or copied as bytes; migration ran on snapshot)"
