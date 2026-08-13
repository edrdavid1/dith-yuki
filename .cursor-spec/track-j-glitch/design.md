# Design: Track J — Glitch correctness

## Overview

Rewrite `filters/glitch.rs` apply to the A/C coordinate contract. Do not add a second Glitch kind.

| ID | Deliverable |
|----|-------------|
| **J1** | GlobalCoord shift field + halo-safe reads |
| **J2** | Intensity maps into `≤ HALO` px |
| **J3** | 2×2 seam tests + K-compliant UI |

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| As-built | Keep `GlitchType::{RGBShift, BlockDisplace}`, XorShift64, seed u64, intensity 0..1 |
| Halo | v1 cap offsets to `HALO` (2). Same product compromise as Glow |
| PRNG key | `seed XOR f(global_x, global_y, level)` for the **destination** pixel (RGB Shift) or **destination block origin in global space** (Block Displace). Not `coord.x << 16` |
| Block size | Keep 16px; origin = `floor(gx / 16) * 16` in **global** pixels |
| Hardcoded 260 | Use `TILE_SIZE + 2*HALO` / iterate local including halo like other filters |
| Wide read | Future; not this track |
| GPU | No |

---

## Current bug

`apply_rgb_shift` / `apply_block_displace` use local x,y in `0..260`, clamp sources to 259, and seed from tile indices. Adjacent tiles draw independent random fields and cannot sample across the seam.

## Target apply (RGB Shift)

For each local `(lx, ly)` including halo:

1. `g = GlobalCoordSigned::from_local_with_halo(coord, lx, ly, HALO)`
2. `rng = XorShift64(mix(seed, g.x, g.y, level))` — or hash to a per-pixel shift without burning many rng calls; lock: **one mix → three channel shifts** so it’s cheap and deterministic
3. `shift_* = scale(rng, intensity) * HALO` as i32
4. Source = `g + (shift, 0)` (or 2D if desired — **lock: existing is X-only for RGB channels**; keep X-only)
5. Convert source global → local; if outside buffer, clamp to **halo edge of this tile buffer** (inevitable at document edge; at internal tile edge, HALO cap means neighbor pixel is inside this tile’s halo)

Because cap ≤ HALO, internal tile seams have the neighbor pixel in-halo. That is why the cap exists.

## Block Displace

Displacement vector per global block origin. Copy block samples with the same halo-safe global read. Unfilled dest pixels: leave pre (or zero then composite — **lock: start from copy of input tile**, then overwrite displaced blocks, matching a less-destructive read of current code which writes into a fresh tile and can leave holes — **lock v1: start from `pre` copy** so holes don’t appear).

---

## Testing

| Test | Assert |
|------|--------|
| Determinism | same seed, same tile (keep) |
| Seam RGB | 2×2, intensity 1, HALO-capped |
| Seam blocks | block straddling x=256 |
| Intensity 0 | copy |
| Cross-tile PRNG | two tiles’ shared-edge pixels use shifts consistent with global x |

---

## Future

- Neighbor-tile fetch for displacements > HALO
- Extra glitch kinds
