# Late-tiles mechanisms — audit vs PR #67 + follow-ups

**PR:** [#67](https://github.com/edrdavid1/dith-yuki/pull/67) `diagnose/h4-stale-gen-z9`  
**Date:** 2026-10-21  
**Constraint:** no file-format changes (honored).

## Mechanism map

| # | Contract | Closed in #67? | Edits required after #67 tip |
| --- | --- | --- | --- |
| **(1)** | Processed/Composite stamp = `content_gen`; Fresh only when stamp matches live (`generation >= content_epoch` with epoch mirrored to live) | **Mostly yes** (`content_gen`, `set_content_epoch`, `tile_fresh`, serve Fresh/Stale) | Serve unit test `clean_behind_content_gen_serves_stale_not_fresh`; dirty-collection epoch check (local); comments |
| **(2)** | `tile-ready` carries **body** stamp of a real inserted/fresh tile; no emit / no FE `maxKnownGen` bump on missed insert | **No** — #67 still forged `ready_gen = body.max(live_content)` when any cache entry existed | `tile_ready_emit_gen`; worker + `pending_tiles` gates; unit test that red-documents the forge |
| **(3)** | L1+ parent stamp = `min(children content stamps)`; rebuild clean parent on divergence; palette + zoom-out-to-fit, all levels | **No** — parents stamped with live `content_gen` | `pyramid_min_child_generation` / `pyramid_parent_needs_rebuild`; pyramid insert + `composite_needs_compute`; Z11 expand harness |

### Red on commit before fix

| Test | Red commit | Failure mode |
| --- | --- | --- |
| `tile_fresh` only `!dirty` | `f6922ac` (Step A) | clean gen=1 stays “fresh” after content_epoch=2 |
| Forge `body.max(live)` | `fd1c09a` tip (#67) / pre-worker fix | dirty body=5, live=7 → emit gen=7 |
| Parent stamp = live content | #67 tip | clean parent gen=9 with children min=6 stays “done” |
| Serve Fresh when stamp &lt; live | pre-Step-B | Fresh(3) while live content=5 |

### Green after

| Test | Status |
| --- | --- |
| `ed::tile_fresh_rejects_behind_content_epoch` | pass |
| `ed::tile_ready_emit_gen_skips_missed_insert_forge` | pass (asserts forge would be 7, emit is `None`) |
| `pyramid::pyramid_min_child_generation_is_min_of_fresh_children` | pass |
| `pyramid::pyramid_parent_needs_rebuild_when_clean_but_diverges` | pass |
| `tile_serve::clean_behind_content_gen_serves_stale_not_fresh` | pass |

```bash
cargo test -p engine-tiles --lib -- tile_ready_emit_gen pyramid_min_child pyramid_parent_needs_rebuild tile_fresh_rejects
cargo test -p dither clean_behind_content_gen_serves_stale
```

## Z11 expand

New ignored harness (1920×1080 for soak cost):

1. palette pulse  
2. `pixel_size`  
3. `levels`  
4. `mode` (Bayer ↔ FS) then back to FS  
5. soft_proof enable toggle (`content_gen`)  
6. **zoom out to fit**  
7. fingerprint / freshness / pyramid divergence on **all** visible levels  

| Test | Command |
| --- | --- |
| Smoke | `cargo test -p dither --release z11_expand_matrix_smoke -- --ignored --nocapture --test-threads=1` |
| ×50 release | `cargo test -p dither --release z11_expand_50_series -- --ignored --nocapture --test-threads=1` |
| ×50 dev | `cargo test -p dither z11_expand_50_series -- --ignored --nocapture --test-threads=1` |

Artifacts: `profiling/results/2026-10-21/h4_z11/z11_expand_50_{dev,release}.md`

| Matrix | Result |
| --- | ---: |
| `z11_expand_matrix_smoke` (dev) | **ok** (~5.5s) |
| `z11_expand_50_series` release | **fail=0** / 50 (~244s) |
| `z11_expand_50_series` dev | **fail=0** / 50 |

## What #67 already closed (keep)

- Monotonic `content_gen` + `X-Tile-Gen` third segment  
- Palette/soft-proof `content-changed` + FE parse of content segment  
- `tile_fresh` + cache `content_epoch` (left-column ED strip)  
- Z11 palette single/drag/series fingerprint quiet (L0)  
- WebView `?c=` cache bust  

## Residual

- Z11 expand ×50 is ignored soak (not CI default); run locally/nightly.  
- Soft-proof bumps `content_epoch` and can mark Processed behind-epoch (pre-existing #67 path); expand returns to FS before zoom-fit.  
- FE keeps prior frame as placeholder during recompute (blank-canvas regression fixed separately).  
