# Sequential Tile Render Fix — Bugfix Design

## Overview

Tiles in the viewport render in a visible scan-line pattern (top-left → bottom-right) instead of appearing from the center outward. The root cause is twofold: (1) the frontend Web Worker serializes tile fetch requests using sequential `await` in a for-loop, and (2) the backend enqueues tiles within a priority bucket in nested-loop raster order (FIFO in SegQueue = scan-line dequeue). The fix parallelizes frontend fetches with `Promise.all` and sorts backend tile enqueue order by manhattan distance from viewport center.

## Glossary

- **Bug_Condition (C)**: A batch of multiple tiles is requested — both the sequential fetch and raster-order dequeue manifest only when more than one tile is in the batch
- **Property (P)**: All tiles in a batch are fetched concurrently (frontend) and dequeued in center-out order (backend)
- **Preservation**: Single-tile `fetch-tile` requests, message format (`tile-decoded`, `tile-pending`, `tile-error`), cross-bucket priority ordering, and `classify_priority` logic remain unchanged
- **tileWorker.ts**: Web Worker at `frontend/src/workers/tileWorker.ts` that fetches tiles from the `tile://` protocol and decodes RGBA8 → ImageBitmap
- **set_viewport**: Tauri IPC command in `src-tauri/src/viewport.rs` that computes visible tiles and enqueues dirty ones into the Scheduler
- **compute_visible_tiles**: Pure function in `viewport.rs` that returns tile coordinates for the current viewport — iterates in row-major nested-loop order
- **SegQueue**: Crossbeam lock-free FIFO queue used for each priority bucket in the Scheduler
- **Manhattan distance**: `|tile.x - center_x| + |tile.y - center_y|` — used for center-out ordering

## Bug Details

### Bug Condition

The bug manifests when a batch of multiple tiles is requested via the `request-tiles` worker message. The frontend serializes fetches (each `fetchAndDecodeTile` must complete before the next starts), and the backend processes tiles in the order they were enqueued — which is row-major raster order from `compute_visible_tiles`'s nested loops.

**Formal Specification:**
```
FUNCTION isBugCondition(input)
  INPUT: input of type TileBatchRequest
  OUTPUT: boolean

  RETURN input.tiles.length > 1
END FUNCTION
```

### Examples

- **3×3 viewport grid**: User pans to center of a large image. Tiles render sequentially top-left to bottom-right over ~300ms instead of all 9 appearing within ~50ms from the center outward.
- **5×4 viewport at zoom 0.5**: 20 tiles load one-by-one in scan-line order. Center tiles at (2,1), (2,2) render 10th–12th, not 1st–2nd.
- **Single tile request** (`fetch-tile` message): Works correctly — no serialization issue because only 1 fetch occurs.
- **Edge case — 1×1 viewport**: Only 1 tile visible, bug condition does not hold, no observable defect.

## Expected Behavior

### Preservation Requirements

**Unchanged Behaviors:**
- Single-tile `fetch-tile` messages must continue to fetch and decode immediately
- `tile-decoded` messages must continue to transfer ImageBitmap with the same `{ type, key, bitmap }` shape
- `tile-pending` (HTTP 202) messages must continue to post back for re-request on `tile-ready` event
- `tile-error` messages must continue to include error details on failure
- Cross-bucket priority ordering (Immediate > ViewportCenter > ViewportEdge > Prefetch) must be preserved
- `classify_priority` must continue to classify tiles as ViewportCenter or ViewportEdge based on the inner-50% rule
- `compute_visible_tiles` must continue to return the correct set of tile coordinates
- `compute_prefetch_ring` must continue to produce the one-tile-wide ring
- Worker pool staleness check (generation comparison) must remain unchanged

**Scope:**
All inputs that do NOT involve multi-tile batch requests should be completely unaffected by this fix. This includes:
- Single-tile `fetch-tile` messages (no serialization issue with 1 tile)
- Prefetch ring computation logic
- Tile cache get/insert operations
- Document mutation and invalidation flow
- Export pipeline (does not use Web Worker batches)

## Hypothesized Root Cause

Based on code analysis, the confirmed root causes are:

1. **Frontend: Sequential `await` in for-loop** (`tileWorker.ts` lines 126–128):
   ```typescript
   for (const tile of msg.tiles) {
     await fetchAndDecodeTile(msg.docId, tile);
   }
   ```
   Each tile fetch must resolve (network round-trip + decode) before the next begins. With N tiles, total latency is N × single-tile-latency instead of max(single-tile-latency) for parallel.

2. **Backend: Raster-order enqueue in `set_viewport`** (`viewport.rs` lines 240–255):
   The `for coord in &visible` loop iterates `visible` tiles in the order returned by `compute_visible_tiles`, which uses nested `for ty in min_ty..max_ty { for tx in min_tx..max_tx }` — producing row-major/raster order. Since `SegQueue` is FIFO within a priority bucket, tiles dequeue in scan-line order regardless of distance from center.

3. **No secondary sorting within priority bucket**: The `classify_priority` function correctly separates ViewportCenter from ViewportEdge, but within each bucket multiple tiles still dequeue in raster order (FIFO). There is no distance-based sorting before enqueue.

4. **Combined effect**: Even though the backend correctly classifies center tiles as higher priority, the frontend serializes their delivery — so raster ordering within a bucket directly becomes visible rendering order.

## Correctness Properties

Property 1: Bug Condition - Parallel Tile Fetch

_For any_ `request-tiles` batch where `tiles.length > 1`, the fixed `tileWorker.ts` SHALL initiate all fetch requests concurrently (all fetch start times within a negligible epsilon) and post each decoded result back to the main thread independently as soon as it resolves, without waiting for other tiles in the batch.

**Validates: Requirements 2.1, 2.3**

Property 2: Bug Condition - Center-Out Enqueue Order

_For any_ call to `set_viewport` where more than one tile needs recomputation within the same priority bucket, the fixed `set_viewport` SHALL enqueue tiles in ascending manhattan distance from the viewport center, so that center tiles are dequeued and processed before edge tiles within the same bucket.

**Validates: Requirements 2.2, 2.3**

Property 3: Preservation - Single Tile Fetch Behavior

_For any_ `fetch-tile` message (single tile request), the fixed code SHALL produce exactly the same behavior as the original code — same fetch, same decode, same message format (`tile-decoded`, `tile-pending`, `tile-error`).

**Validates: Requirements 3.1, 3.2, 3.3, 3.6**

Property 4: Preservation - Cross-Bucket Priority Ordering

_For any_ set of enqueued tiles across different priority buckets, the fixed scheduler SHALL continue to dequeue Immediate before ViewportCenter before ViewportEdge before Prefetch, preserving the existing cross-bucket ordering.

**Validates: Requirements 3.4, 3.5**

## Fix Implementation

### Changes Required

**File**: `frontend/src/workers/tileWorker.ts`

**Function**: `self.onmessage` handler for `request-tiles`

**Specific Changes**:
1. **Replace sequential loop with `Promise.all`**: Instead of `for...await` over tiles, map all tiles to concurrent `fetchAndDecodeTile` promises and await them together. Each promise independently calls `postMessage` upon resolution (the existing `fetchAndDecodeTile` function already posts results individually), so results stream back as they complete.

   ```typescript
   // Before (sequential):
   for (const tile of msg.tiles) {
     await fetchAndDecodeTile(msg.docId, tile);
   }

   // After (parallel):
   await Promise.all(
     msg.tiles.map(tile => fetchAndDecodeTile(msg.docId, tile))
   );
   ```

2. **No change to `fetchAndDecodeTile`**: This function already handles a single tile end-to-end (fetch → decode → postMessage), so parallelizing at the call-site is sufficient.

---

**File**: `src-tauri/src/viewport.rs`

**Function**: `set_viewport`

**Specific Changes**:
3. **Sort visible tiles by manhattan distance before enqueuing**: After calling `compute_visible_tiles` (which returns tiles in raster order), sort the resulting vector by ascending manhattan distance from the viewport center before iterating for scheduling.

   ```rust
   // Compute viewport center in tile coordinates
   let center_x = if visible.is_empty() { 0.0 } else {
       let min_x = visible.iter().map(|t| t.x).min().unwrap();
       let max_x = visible.iter().map(|t| t.x).max().unwrap();
       (min_x + max_x) as f64 / 2.0
   };
   let center_y = if visible.is_empty() { 0.0 } else {
       let min_y = visible.iter().map(|t| t.y).min().unwrap();
       let max_y = visible.iter().map(|t| t.y).max().unwrap();
       (min_y + max_y) as f64 / 2.0
   };

   // Sort by manhattan distance from center (center-out ordering)
   visible.sort_by(|a, b| {
       let dist_a = ((a.x as f64 - center_x).abs() + (a.y as f64 - center_y).abs());
       let dist_b = ((b.x as f64 - center_x).abs() + (b.y as f64 - center_y).abs());
       dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
   });
   ```

4. **Make `visible` mutable**: Change `let visible = ...` to `let mut visible = ...` to allow in-place sorting.

5. **No change to `compute_visible_tiles`**: The function remains a pure coordinate computation. Sorting is applied in `set_viewport` after the call, keeping concerns separated.

6. **No change to `Scheduler` or `SegQueue`**: FIFO semantics within a bucket are correct — we simply enqueue in the desired order.

7. **No change to prefetch ring ordering**: Prefetch tiles are lower priority and not visible; their dequeue order is less perceptually important. However, the same sort can optionally be applied for consistency.

## Testing Strategy

### Validation Approach

The testing strategy follows a two-phase approach: first, surface counterexamples that demonstrate the bug on unfixed code, then verify the fix works correctly and preserves existing behavior.

### Exploratory Bug Condition Checking

**Goal**: Surface counterexamples that demonstrate the bug BEFORE implementing the fix. Confirm or refute the root cause analysis. If we refute, we will need to re-hypothesize.

**Test Plan**: Write tests that verify fetch timing (frontend) and dequeue ordering (backend) on the current unfixed code to observe the sequential/raster-order behavior.

**Test Cases**:
1. **Frontend Sequential Fetch Test**: Mock `fetch` and record start times for a batch of 5 tiles. Assert that start times are staggered (not concurrent) — will fail if fetches are parallel, confirming sequential behavior on unfixed code.
2. **Backend Raster Order Test**: Call `set_viewport` with a 4×4 visible tile grid, then dequeue all tasks from the ViewportCenter queue. Assert that dequeue order is NOT center-out (raster order observed) — confirms FIFO raster behavior on unfixed code.
3. **Combined Perceptual Test**: Measure time-to-first-center-tile vs time-to-last-tile in a 3×3 grid. On unfixed code, center tile appears ~5th (middle of 9), not 1st.

**Expected Counterexamples**:
- Frontend: fetch start time for tile[i+1] ≈ fetch start time for tile[i] + RTT (sequential)
- Backend: dequeue order matches `compute_visible_tiles` iteration order (row-major), not sorted by distance

### Fix Checking

**Goal**: Verify that for all inputs where the bug condition holds, the fixed function produces the expected behavior.

**Pseudocode:**
```
FOR ALL input WHERE isBugCondition(input) DO
  // Frontend: all fetches initiated concurrently
  startTimes := recordFetchStartTimes(input.tiles)
  ASSERT max(startTimes) - min(startTimes) < EPSILON

  // Backend: dequeue order is center-out
  center := computeViewportCenter(input.viewport)
  dequeueOrder := recordDequeueOrder(scheduler, priorityBucket)
  FOR i FROM 0 TO dequeueOrder.length - 2 DO
    dist_i := manhattanDistance(dequeueOrder[i], center)
    dist_next := manhattanDistance(dequeueOrder[i+1], center)
    ASSERT dist_i <= dist_next
  END FOR
END FOR
```

### Preservation Checking

**Goal**: Verify that for all inputs where the bug condition does NOT hold, the fixed function produces the same result as the original function.

**Pseudocode:**
```
FOR ALL input WHERE NOT isBugCondition(input) DO
  ASSERT originalFunction(input) = fixedFunction(input)
END FOR
```

**Testing Approach**: Property-based testing is recommended for preservation checking because:
- It generates many random viewport configurations and verifies cross-bucket priority ordering
- It catches edge cases (empty viewports, single-tile grids, viewports at document edges)
- It provides strong guarantees that behavior is unchanged for non-batch inputs

**Test Plan**: Observe behavior on UNFIXED code first for single-tile requests, cross-bucket ordering, and tile set correctness, then write property-based tests capturing that behavior.

**Test Cases**:
1. **Single Tile Fetch Preservation**: Verify `fetch-tile` message produces identical `tile-decoded` / `tile-pending` / `tile-error` messages before and after fix
2. **Cross-Bucket Priority Preservation**: Generate random mixes of Immediate, ViewportCenter, ViewportEdge, and Prefetch tasks; verify dequeue always respects priority ordering
3. **Visible Tile Set Preservation**: For random viewport parameters, verify the same set of TileCoords is computed (only order changes, not membership)
4. **Classify Priority Preservation**: For random tile coordinates and visible sets, verify `classify_priority` returns the same result before and after fix

### Unit Tests

- Test that `Promise.all` branch in `tileWorker.ts` posts messages for all tiles in a batch
- Test that individual `fetchAndDecodeTile` errors don't abort other tiles in the batch
- Test `set_viewport` enqueue order is sorted by manhattan distance for various grid sizes
- Test edge cases: 1×1 grid (no sorting needed), equal-distance tiles maintain stable order
- Test that `compute_visible_tiles` return value (set) is unchanged

### Property-Based Tests

- Generate random viewport rectangles (zoom, pan, size) and verify enqueue order is non-decreasing manhattan distance from center (Rust proptest)
- Generate random tile batches and verify all fetch promises resolve independently (TypeScript fast-check)
- Generate random priority mixes and verify cross-bucket dequeue ordering (Rust proptest)
- Generate random tile coordinates and verify `classify_priority` is unchanged

### Integration Tests

- Full Tauri integration: load image → set viewport → verify tiles arrive at frontend in center-out temporal order
- Verify that after fix, center-of-viewport tiles are decoded before edge tiles in a real tile:// fetch scenario
- Test that `tile-ready` events still trigger correct re-fetch for 202 responses
