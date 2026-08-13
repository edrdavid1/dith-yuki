# Implementation Plan: Track J — Glitch correctness

План: [requirements.md](./requirements.md), [design.md](./design.md).
Источник: ROADMAP J; as-built `filters/glitch.rs`.

**Gate:** none. **Locked:** GlobalCoord; offset ≤ HALO; keep kinds/XorShift64/seed; start Block Displace from pre copy.

**Порядок:** J0 → J1 → J2 → J3.

---

## 0. Baseline

- [x] 0.1 Inventory `glitch.rs`, `apply.rs` Glitch arm, `GlitchSettings.tsx`, HALO constant
  - _Requirements: 1_
  - As-built (before rewrite): local `0..260`, PRNG `seed XOR (level) XOR (tile.x<<16) XOR (tile.y<<32)`, max shift `20 * intensity`, clamp sources to `0..259`. `HALO = 2`, `TILE_SIZE = 256`. UI: Slider intensity + raw `<input type="number">` seed.

- [x] 0.2 Link docs
  - _Requirements: n/a_
  - `ARCHITECTURE.md` §5.6; [tech-debit.md](../tech-debit.md) Track J.

---

## 1. J1 — Coordinate rewrite

- [x] 1.1 RGB Shift via `GlobalCoordSigned` + mix(seed, gx, gy)
  - _Requirements: 1.1–1.3_

- [x] 1.2 Block Displace via global block origin; dest starts as pre copy
  - _Requirements: 1, 4.2_

- [x] 1.3 Replace hardcoded 260 with TILE_SIZE/HALO
  - _Requirements: 1.1_

---

## 2. J2 — Halo cap

- [x] 2.1 Map intensity → px offset in `0..=HALO`
  - _Requirements: 2.1–2.2_

---

## 3. J3 — Tests + UI

- [x] 3.1 Keep determinism / intensity 0 tests
  - _Requirements: 3_

- [x] 3.2 New 2×2 seam tests RGB + Block Displace
  - _Requirements: 4.1–4.2_

- [x] 3.3 GlitchSettings uses Track K Slider / NumberInput
  - _Requirements: 4.3_

---

## Definition of Done

- [x] No local-only PRNG key
- [x] Offset ≤ HALO
- [x] 2×2 seam tests green
- [x] Existing serde/UI kinds unchanged
