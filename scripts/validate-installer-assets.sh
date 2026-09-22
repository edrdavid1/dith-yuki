#!/usr/bin/env bash
# Validate NSIS installer branding assets (SPEC_dither_installer_branding.md §1.4).
# Fails the build on mismatch — no silent skip, no auto-reconvert.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ASSET_DIR="$ROOT/src-tauri/icons/win-setup"

SIDEBAR="$ASSET_DIR/sidebarImage.bmp"
HEADER="$ASSET_DIR/headerImage.bmp"
UNHEADER="$ASSET_DIR/uninstallerHeaderImage.bmp"
ICO="$ASSET_DIR/installerIcon.ico"

fail() {
  echo "validate-installer-assets: $*" >&2
  exit 1
}

for f in "$SIDEBAR" "$HEADER" "$UNHEADER" "$ICO"; do
  [[ -f "$f" ]] || fail "missing required file: $f"
done

python3 - "$SIDEBAR" "$HEADER" "$UNHEADER" "$ICO" <<'PY'
import struct
import sys
from pathlib import Path

errors: list[str] = []


def check_bmp(path: Path, expected_w: int, expected_h: int) -> None:
    data = path.read_bytes()
    if len(data) < 54:
        errors.append(f"{path}: too small to be a BMP")
        return
    if data[:2] != b"BM":
        errors.append(f"{path}: not a BMP (missing BM signature)")
        return
    dib_size = struct.unpack_from("<I", data, 14)[0]
    if dib_size < 40:
        errors.append(f"{path}: unexpected DIB header size {dib_size} (want BITMAPINFOHEADER ≥ 40)")
        return
    width, height = struct.unpack_from("<ii", data, 18)
    planes, bpp = struct.unpack_from("<HH", data, 26)
    compression = struct.unpack_from("<I", data, 30)[0]
    abs_h = abs(height)
    if (width, abs_h) != (expected_w, expected_h):
        errors.append(
            f"{path}: size {width}×{abs_h} px, expected {expected_w}×{expected_h} px"
        )
    if bpp != 24:
        errors.append(f"{path}: bit depth {bpp}, expected 24 (no alpha / no palette)")
    if compression != 0:
        errors.append(f"{path}: compression={compression}, expected 0 (BI_RGB, uncompressed)")
    if planes != 1:
        errors.append(f"{path}: planes={planes}, expected 1")


def check_ico(path: Path, required: set[tuple[int, int]]) -> None:
    data = path.read_bytes()
    if len(data) < 6:
        errors.append(f"{path}: too small to be an ICO")
        return
    reserved, ico_type, count = struct.unpack_from("<HHH", data, 0)
    if reserved != 0 or ico_type != 1:
        errors.append(f"{path}: not a valid ICO (reserved={reserved}, type={ico_type})")
        return
    if count < 1:
        errors.append(f"{path}: ICO has no image entries")
        return
    found: set[tuple[int, int]] = set()
    off = 6
    for i in range(count):
        if off + 16 > len(data):
            errors.append(f"{path}: truncated ICO directory at entry {i}")
            break
        w, h, _colors, _res, _planes, _bitcount, _nbytes, _img_off = struct.unpack_from(
            "<BBBBHHII", data, off
        )
        w = 256 if w == 0 else w
        h = 256 if h == 0 else h
        found.add((w, h))
        off += 16
    missing = sorted(required - found)
    if missing:
        miss = ", ".join(f"{w}×{h}" for w, h in missing)
        have = ", ".join(f"{w}×{h}" for w, h in sorted(found))
        errors.append(f"{path}: missing required sizes [{miss}]; have [{have}]")


sidebar, header, unheader, ico = map(Path, sys.argv[1:5])
check_bmp(sidebar, 164, 314)
check_bmp(header, 150, 57)
check_bmp(unheader, 150, 57)
check_ico(ico, {(16, 16), (32, 32), (48, 48), (256, 256)})

if errors:
    print("validate-installer-assets: FAILED", file=sys.stderr)
    for e in errors:
        print(f"  - {e}", file=sys.stderr)
    sys.exit(1)

print("validate-installer-assets: OK")
print(f"  sidebar: {sidebar.name} 164×314 24-bit BI_RGB")
print(f"  header:  {header.name} 150×57 24-bit BI_RGB")
print(f"  unheader:{unheader.name} 150×57 24-bit BI_RGB")
print(f"  icon:    {ico.name} contains 16/32/48/256")
PY
