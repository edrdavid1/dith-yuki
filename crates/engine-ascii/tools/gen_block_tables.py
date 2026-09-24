#!/usr/bin/env python3
"""Generate `src/symbols/block_tables.rs` from Unicode data files.

Usage:
    curl -sSfLO https://www.unicode.org/Public/16.0.0/ucd/UnicodeData.txt
    curl -sSfLO https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT
    python3 tools/gen_block_tables.py UnicodeData.txt CP437.TXT > src/symbols/block_tables.rs

CP437: bytes 0x80-0xFF come from the official Microsoft mapping. That file
maps 0x00-0x1F and 0x7F to control codes; the IBM PC displays graphic
glyphs there, listed in `CP437_GRAPHIC_LOW`. 0x00 is rendered blank.

Sextant / octant dot patterns are read from the character names
(`BLOCK SEXTANT-1235`, `BLOCK OCTANT-3`), which encode the filled sub-cells
exactly. Patterns without a `BLOCK SEXTANT` / `BLOCK OCTANT` name are
represented in Unicode by older block characters; those equivalences are
listed explicitly in `EQUIVALENTS` and every pattern must be covered exactly
once or the script fails.

Sub-cell numbering (both tables): row-major, left column first.
    sextant: 1 2 / 3 4 / 5 6          octant: 1 2 / 3 4 / 5 6 / 7 8
Bit `n - 1` is set when sub-cell `n` is filled.
"""

import re
import sys


def bits(digits: str) -> int:
    value = 0
    for d in digits:
        value |= 1 << (int(d) - 1)
    return value


# Older characters that already encode a sub-cell pattern. Digits use the
# numbering of the respective table.
SEXTANT_EQUIVALENTS = {
    "": 0x0020,        # SPACE
    "135": 0x258C,     # LEFT HALF BLOCK
    "246": 0x2590,     # RIGHT HALF BLOCK
    "123456": 0x2588,  # FULL BLOCK
}

OCTANT_EQUIVALENTS = {
    "": 0x0020,          # SPACE
    "12345678": 0x2588,  # FULL BLOCK
    "1234": 0x2580,      # UPPER HALF BLOCK
    "5678": 0x2584,      # LOWER HALF BLOCK
    "1357": 0x258C,      # LEFT HALF BLOCK
    "2468": 0x2590,      # RIGHT HALF BLOCK
    "57": 0x2596,        # QUADRANT LOWER LEFT
    "68": 0x2597,        # QUADRANT LOWER RIGHT
    "13": 0x2598,        # QUADRANT UPPER LEFT
    "135678": 0x2599,    # QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT
    "1368": 0x259A,      # QUADRANT UPPER LEFT AND LOWER RIGHT
    "123457": 0x259B,    # QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT
    "123468": 0x259C,    # QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT
    "24": 0x259D,        # QUADRANT UPPER RIGHT
    "2457": 0x259E,      # QUADRANT UPPER RIGHT AND LOWER LEFT
    "245678": 0x259F,    # QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT
    "78": 0x2582,        # LOWER ONE QUARTER BLOCK
    "345678": 0x2586,    # LOWER THREE QUARTERS BLOCK
    "12": 0x1FB82,       # UPPER ONE QUARTER BLOCK
    "123456": 0x1FB85,   # UPPER THREE QUARTERS BLOCK
    "1": 0x1CEA8,        # LEFT HALF UPPER ONE QUARTER BLOCK
    "2": 0x1CEAB,        # RIGHT HALF UPPER ONE QUARTER BLOCK
    "7": 0x1CEA3,        # LEFT HALF LOWER ONE QUARTER BLOCK
    "8": 0x1CEA0,        # RIGHT HALF LOWER ONE QUARTER BLOCK
    "35": 0x1FBE6,       # MIDDLE LEFT ONE QUARTER BLOCK
    "46": 0x1FBE7,       # MIDDLE RIGHT ONE QUARTER BLOCK
}


# IBM PC glyphs for CP437 bytes 0x00-0x1F, then 0x7F.
CP437_GRAPHIC_LOW = [
    0x0020, 0x263A, 0x263B, 0x2665, 0x2666, 0x2663, 0x2660, 0x2022,
    0x25D8, 0x25CB, 0x25D9, 0x2642, 0x2640, 0x266A, 0x266B, 0x263C,
    0x25BA, 0x25C4, 0x2195, 0x203C, 0x00B6, 0x00A7, 0x25AC, 0x21A8,
    0x2191, 0x2193, 0x2192, 0x2190, 0x221F, 0x2194, 0x25B2, 0x25BC,
]
CP437_GRAPHIC_DEL = 0x2302


def load_cp437(path: str, names: dict) -> list:
    table = {}
    with open(path, encoding="latin-1") as fh:
        for line in fh:
            line = line.split("#", 1)[0].strip()
            if not line.startswith("0x"):
                continue
            byte, cp = (int(v, 16) for v in line.split()[:2])
            table[byte] = cp
    for byte, cp in enumerate(CP437_GRAPHIC_LOW):
        table[byte] = cp
    table[0x7F] = CP437_GRAPHIC_DEL
    if sorted(table) != list(range(256)):
        sys.exit("CP437 mapping incomplete")
    for byte, cp in table.items():
        if cp != 0x20 and cp not in names:
            sys.exit(f"CP437 0x{byte:02X} -> U+{cp:04X} not in UnicodeData")
    return [table[b] for b in range(256)]


def load_names(path: str) -> dict:
    names = {}
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            fields = line.split(";")
            names[int(fields[0], 16)] = fields[1]
    return names


def build(names: dict, prefix: str, equivalents: dict, cells: int) -> list:
    pattern_re = re.compile(rf"^BLOCK {prefix}-([1-{cells}]+)$")
    table = {}
    for cp, name in names.items():
        m = pattern_re.match(name)
        if m:
            p = bits(m.group(1))
            if p in table:
                sys.exit(f"duplicate {prefix} pattern {p:#x}: U+{cp:04X}")
            table[p] = cp
    for digits, cp in equivalents.items():
        if cp not in names and cp != 0x20:
            sys.exit(f"equivalent U+{cp:04X} missing from UnicodeData")
        p = bits(digits)
        if p in table:
            sys.exit(f"{prefix} pattern {p:#x} covered twice (U+{table[p]:04X}, U+{cp:04X})")
        table[p] = cp
    full = 1 << cells
    missing = [p for p in range(full) if p not in table]
    if missing:
        listing = ", ".join(
            "".join(str(i + 1) for i in range(cells) if p >> i & 1) or "(empty)"
            for p in missing
        )
        sys.exit(f"{prefix}: patterns without a character: {listing}")
    return [table[p] for p in range(full)]


def emit(name: str, doc: str, cps: list) -> str:
    rows = []
    for i in range(0, len(cps), 8):
        rows.append("    " + " ".join(f"'\\u{{{cp:04X}}}'," for cp in cps[i : i + 8]))
    return f"/// {doc}\npub const {name}: [char; {len(cps)}] = [\n" + "\n".join(rows) + "\n];\n"


def main() -> None:
    names = load_names(sys.argv[1])
    sextants = build(names, "SEXTANT", SEXTANT_EQUIVALENTS, 6)
    octants = build(names, "OCTANT", OCTANT_EQUIVALENTS, 8)
    cp437 = load_cp437(sys.argv[2], names)
    print("// @generated by tools/gen_block_tables.py from UnicodeData.txt (Unicode 16.0.0)")
    print("// and MICSFT/PC/CP437.TXT. Do not edit by hand; regenerate instead.")
    print()
    print(emit("SEXTANTS", "Sextant pattern (bit n-1 = sub-cell n, 2x3) -> character.", sextants))
    print(emit("OCTANTS", "Octant pattern (bit n-1 = sub-cell n, 2x4) -> character.", octants))
    print(emit("CP437", "IBM PC code page 437 byte -> character (graphic glyphs for 0x01-0x1F, 0x7F).", cp437), end="")


if __name__ == "__main__":
    main()
