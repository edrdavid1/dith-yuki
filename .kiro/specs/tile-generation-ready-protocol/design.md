# Design: Tile generation / ready protocol

## Problem

TICKET-1 made `insert_fresh_gen` refuse a stale write. That is necessary and insufficient.

```
slider → increment Doc_Gen → invalidate dirty → worker compute(gen=K)
                                    ↘ slow worker compute(gen=K-1)
insert_fresh_gen(K-1) → false, cache unchanged
protocol get: !dirty, pixels of gen K-1 or K
  if Entry_Gen < Doc_Gen but dirty was cleared by a previous successful insert
  → HTTP 200  == Stale_200  → canvas treats tile as “updated”
```

Frontend `shouldCommitTileRefresh` then either:

- waits forever if one tile never gets a current decoded (full freeze on old frame), or
- if A is applied without C, commits a mix of good tiles and Stale_200 (silent corruption — TICKET-1 class).

`insert_fresh_gen == false` today: Occupied, `cached.generation > incoming`, **return false, no dirty, no enqueue**. If live Doc_Gen is already ahead of cache, nothing will produce the current frame unless something else invalidates.

## Order

```
E  protocol Ready = !dirty && Entry_Gen >= Doc_Gen
C  client drop decoded.rev < currentRev
6  TICKET-1 hole: reject => ensure current gen will be computed
A  batch timeout (cannot start before C)
D  measure 202-exhausted after E+C+A; only then consider longer retries + rev-cancel
F  only if pan-back stale survives E+C
B, G  never in this spec
```

E and C are one invariant: **the server must not lie that a tile is current; the client must not paint a response it knows is old.**

## E — Protocol

`handle_tile_request` (`src-tauri/src/main.rs`):

```
let doc_gen = snapshot.generations.document_gen.load(Acquire);

if let Some(entry) = cache.entries.get(&key) {
    let ready = !entry.dirty.load(Acquire) && entry.generation >= doc_gen;
    if ready {
        return 200 + body + header X-Tile-Generation: entry.generation
    }
}
// missing, dirty, or Entry_Gen < Doc_Gen
schedule Immediate current-gen task (enqueue_dedup)
return 202
```

Do not serve 200 “because bytes exist”. Dirty with bytes is 202. Fresh-looking bytes with old generation is 202.

`?g=` stays cache-bust for WKWebView only; Ready is decided solely by cache + Doc_Gen.

## C — Client rev

Already: `tileRevRef`, `?g=` on URL, `onDocumentChanged` bumps rev.

Gaps:

1. `tile-decoded` must carry `rev`.
2. `handleWorkerMessage`: if `msg.rev < tileRevRef.current` → `bitmap.close()`, return.
3. Pending map: drop pending entries whose rev is stale when rev bumps (close bitmaps).

Commit rule: only bitmaps with `rev === current` enter pending/displayed.

## п.6 — TICKET-1 closing patch

Call sites of `insert_fresh_gen` in `tile_pipeline.rs` (Processed, Composite, pyramid parent):

On `false`:

1. Read live `Doc_Gen`.
2. If `live > cached.generation` (need a current write that nobody produced): `mark_dirty(key)` + `scheduler.enqueue_dedup(RecomputeTask { generation: live, ... })` + `worker_wake`.
3. If `live == cached.generation`: cache already has the current frame; leave clean.

Do **not** `insert` the rejected pixels.

Extend `insert_fresh_gen_keeps_newer_generation`: after reject of gen 1 vs cache gen 2, simulate live gen 3 (or call a new helper `on_insert_rejected(cache, key, live_gen, scheduler)`) and assert dirty + queued task gen 3.

Treat as **regression/bug** on TICKET-1, not a new perf ticket.

## A — Batch timeout (second)

Constant `COMMIT_WAIT_MS = 100` in `TileCanvas.tsx`.

On first current-rev pending after a rev bump, start a timer. On fire: commit all pending with current rev; leave other displayed keys as-is.

Without C this would paint Stale_200 into the mixed frame — forbidden; A is after E+C.

## D / F / B / G

- **D:** Profile after E+C+A: count worker loops that exit 202 after 5 tries on FS + pixel_size 8, 3k canvas, release. If rare, leave retries. If common, design cancel-on-rev (AbortController or `rev` check each sleep) before extending the cap.
- **F:** Viewport-only schedule stays (Requirement 3 of tile-viewport-rendering). Revisit only with a recorded pan-back stale after this spec.
- **B/G:** Rejected; B is a product decision (seams); G is a bandage if E–A are honest.

## Tests

| Layer | Test |
|-------|------|
| Protocol | Entry gen 1, doc_gen 2, !dirty → 202 not 200 |
| Protocol | Entry gen 2, doc_gen 2, !dirty → 200 + header |
| Protocol | dirty, gen == doc_gen → 202 |
| Cache/worker | TICKET-1 addendum: reject stale insert then live gen bump → dirty + enqueue |
| Frontend | decoded.rev < current dropped (vitest, no bitmap in map) |
| Frontend | A: after timeout, partial current-rev pending commits; stale rev never commits |

## Files (expected)

- `src-tauri/src/main.rs` — Ready check, header
- `crates/engine-tiles/src/cache.rs` — tests; maybe helper for reject follow-up
- `src-tauri/src/tile_pipeline.rs` / `worker.rs` — on insert false
- `src-tauri/src/commands.rs` — schedule already has enqueue_dedup
- `frontend/src/workers/tileWorker.ts` — rev on decoded
- `frontend/src/features/preview/TileCanvas.tsx` — drop stale, timeout
- `frontend/src/features/preview/__tests__/pyramidLevel.test.ts` — commit helpers
- `TICKETS_preview_latency.md` — TICKET-1 addendum + TICKET-7…10

## Risks

- Extra 202 after gen bump is correct; ensure `enqueue_dedup` prevents storm.
- Header might be dropped by custom protocol; if so, client still has `rev` from the request — header is belt-and-suspenders for debugging and future protocol tests.
- Coalesce TICKET-4: pending refresh must still increment Doc_Gen before workers run; Ready uses live snapshot at request time.
