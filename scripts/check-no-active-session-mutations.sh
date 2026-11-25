#!/usr/bin/env bash
# Fail if commands/ call active_session() without an explicit allow marker.
# Mutating IPC must use require_session(doc_id). Read-only chrome may keep
# active_session with: // active-session-ok: <reason>
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

python3 - <<'PY'
import re
import sys
from pathlib import Path

root = Path("src-tauri/src/commands")
bad: list[str] = []
pat = re.compile(r"\.active_session\s*\(")
allow = re.compile(r"active-session-ok:")

for path in sorted(root.rglob("*.rs")):
    if path.name.endswith("tests.rs") or "test" in path.name:
        continue
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    for li, line in enumerate(lines, 1):
        if line.strip().startswith("//"):
            continue
        if not pat.search(line):
            continue
        prev = lines[li - 2] if li >= 2 else ""
        if allow.search(line) or allow.search(prev):
            continue
        bad.append(f"{path}:{li}:{line.strip()}")

if bad:
    print("Forbidden active_session() in commands/ (add // active-session-ok: … or use require_session):")
    print("\n".join(bad[:50]))
    sys.exit(1)
print("OK: no unmarked active_session() in commands/")
PY
