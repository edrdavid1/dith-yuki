#!/usr/bin/env bash
# Fail if production Rust reintroduces magic `doc: 1` / DocumentId::new(1)
# outside #[cfg(test)] (regression class from multi-doc).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

python3 - <<'PY'
import re
import sys
from pathlib import Path

patterns = [re.compile(r"doc:\s*1\b"), re.compile(r"DocumentId::new\(1\)")]
roots = [Path("src-tauri/src"), Path("crates")]
bad: list[str] = []


def strip_cfg_test_blocks(text: str) -> str:
    """Remove #[cfg(test)] mod/fn … { … } (brace-balanced)."""
    out: list[str] = []
    i = 0
    marker = re.compile(
        r"#\[cfg\(test\)\][\s\n]*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:mod|fn)\s+\w+"
    )
    while i < len(text):
        m = marker.search(text, i)
        if not m:
            out.append(text[i:])
            break
        out.append(text[i : m.start()])
        j = m.end()
        while j < len(text) and text[j] != "{":
            if text[j] == ";":
                j += 1
                break
            j += 1
        else:
            if j >= len(text):
                break
            depth = 0
            k = j
            while k < len(text):
                ch = text[k]
                if ch == "{":
                    depth += 1
                elif ch == "}":
                    depth -= 1
                    if depth == 0:
                        k += 1
                        break
                k += 1
            i = k
            continue
        i = j
    return "".join(out)


for root in roots:
    if not root.exists():
        continue
    for path in root.rglob("*.rs"):
        rel = str(path)
        if "/tests/" in rel or path.name.endswith("_test.rs") or "benches" in path.parts:
            continue
        raw = path.read_text(encoding="utf-8", errors="replace")
        if re.search(r"(?m)^#!\[cfg\(test\)\]", raw):
            continue
        body = strip_cfg_test_blocks(raw)
        for li, line in enumerate(body.splitlines(), 1):
            if line.strip().startswith("//"):
                continue
            for pat in patterns:
                if pat.search(line):
                    bad.append(f"{path}:{li}:{line.strip()}")

if bad:
    print("Forbidden magic doc:1 / DocumentId::new(1) in production:")
    print("\n".join(bad[:50]))
    sys.exit(1)
print("OK: no production magic doc:1")
PY
