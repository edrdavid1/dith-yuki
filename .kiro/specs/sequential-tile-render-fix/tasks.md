# Implementation Plan

## Overview

Fix sequential tile rendering (scan-line pattern) by parallelizing frontend tile fetches with `Promise.all` and sorting backend tile enqueue order by manhattan distance from viewport center. Uses the bug condition methodology: explore the bug with property tests, lock in preservation behavior, implement the fix, then validate.

## Tasks

- [x] 1. Write bug condition exploration test
  - **Property 1: Bug Condition** - Sequential Tile Fetch & Raster-Order Enqueue
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bug exists
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate the bug exists in both frontend and backend
  - **Scoped PBT Approach**: Scope properties to concrete failing cases for reproducibility
  - **Frontend (fast-check, vitest)**:
    - Create `frontend/src/workers/__tests__/tileWorker.parallel.test.ts`
    - Mock `fetch` to record call start timestamps (use a small artificial delay per call to make sequentiality observable)
    - Generate arbitrary tile batches of length 2–20 using `fc.array(fc.record({level: fc.nat(3), x: fc.nat(15), y: fc.nat(15)}), {minLength: 2, maxLength: 20})`
    - Property: for all batches where `tiles.length > 1`, assert `max(startTimes) - min(startTimes) < EPSILON` (all fetches initiated concurrently)
    - On UNFIXED code: test FAILS because fetches are sequential (start times are staggered by RTT)
    - Document counterexamples: e.g., "batch of 5 tiles has start time spread of ~500ms instead of <5ms"
  - **Backend (proptest, Rust)**:
    - Create `crates/engine-tiles/tests/bug_condition_test.rs`
    - Add `proptest` to `[dev-dependencies]` in `crates/engine-tiles/Cargo.toml`
    - Generate random visible tile grids (2–10 wide × 2–10 tall) using proptest
    - Extract the sort_tiles_center_out logic into a testable pure function (or test via `compute_visible_tiles` + expected sort order)
    - Property: for all tile grids with >1 tile, after sorting by manhattan distance from center, dequeue order must be non-decreasing distance. On UNFIXED code, the raw `compute_visible_tiles` output is in raster order — assert it IS center-out ordered → test FAILS (confirms raster-order bug)
    - Document counterexamples: e.g., "4×4 grid dequeues (0,0) before (2,2) even though (2,2) is closer to center (1.5,1.5)"
  - Run tests on UNFIXED code
  - **EXPECTED OUTCOME**: Tests FAIL (this is correct - it proves the bug exists)
  - Mark task complete when tests are written, run, and failure is documented
  - _Requirements: 1.1, 1.2, 1.3_

- [x] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - Single-Tile Fetch & Cross-Bucket Priority Ordering
  - **IMPORTANT**: Follow observation-first methodology
  - **Frontend Preservation (fast-check, vitest)**:
    - Create `frontend/src/workers/__tests__/tileWorker.preservation.test.ts`
    - Observe: single `fetch-tile` message for any tile produces exactly one `tile-decoded`, `tile-pending`, or `tile-error` response with correct `{ type, key, ... }` shape
    - Observe: when fetch returns 200 with 262144 bytes → `tile-decoded` with ImageBitmap transferred
    - Observe: when fetch returns 202 → `tile-pending` posted
    - Observe: when fetch throws → `tile-error` with error message
    - Write property-based test: for all single tile coordinates `{level: 0..3, x: 0..15, y: 0..15}`, a `fetch-tile` message produces one outbound message with type matching response status
    - Verify test passes on UNFIXED code
  - **Backend Preservation (proptest, Rust)**:
    - Create `crates/engine-tiles/tests/preservation_test.rs`
    - Observe: cross-bucket priority ordering — Immediate dequeued before ViewportCenter before ViewportEdge before Prefetch regardless of enqueue order
    - Observe: `compute_visible_tiles` returns the same SET of tile coordinates regardless of sorting (set membership preserved)
    - Observe: `classify_priority` returns same result for any tile regardless of intra-bucket ordering
    - Write property-based tests:
      1. For all random mixes of priorities (1–50 tasks across 4 buckets), dequeue order respects priority: `Immediate > ViewportCenter > ViewportEdge > Prefetch`
      2. For all random viewport params `(zoom: 0.01..2.0, x: 0..4096, y: 0..4096, w: 100..2000, h: 100..2000)`, the set of tiles from `compute_visible_tiles` is unchanged (we sort a copy and compare sets)
      3. For all random tile coords and visible sets, `classify_priority` returns the same classification
    - Verify tests pass on UNFIXED code
  - Run tests on UNFIXED code
  - **EXPECTED OUTCOME**: Tests PASS (this confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6_

- [x] 3. Fix for sequential tile rendering (scan-line pattern instead of center-out)

  - [x] 3.1 Implement frontend fix: parallel tile fetching in tileWorker.ts
    - In `frontend/src/workers/tileWorker.ts`, replace the sequential `for...await` loop in the `request-tiles` handler with `Promise.all`
    - Change from:
      ```typescript
      for (const tile of msg.tiles) {
        await fetchAndDecodeTile(msg.docId, tile);
      }
      ```
    - Change to:
      ```typescript
      await Promise.all(
        msg.tiles.map(tile => fetchAndDecodeTile(msg.docId, tile))
      );
      ```
    - No changes to `fetchAndDecodeTile` — it already handles individual tile fetch/decode/postMessage independently
    - Verify that individual tile errors in a batch do not abort other tiles (Promise.all with independent error handling inside each promise — already handled by try/catch in `fetchAndDecodeTile`)
    - _Bug_Condition: isBugCondition(input) where input.tiles.length > 1_
    - _Expected_Behavior: all fetch start times within epsilon for batch requests_
    - _Preservation: single fetch-tile messages unchanged, message format unchanged_
    - _Requirements: 2.1, 2.3_

  - [x] 3.2 Implement backend fix: center-out tile sorting in viewport.rs
    - In `src-tauri/src/viewport.rs` function `set_viewport`, after calling `compute_visible_tiles`, sort the `visible` vector by ascending manhattan distance from viewport center before the scheduling loop
    - Change `let visible = compute_visible_tiles(...)` to `let mut visible = compute_visible_tiles(...)`
    - Compute viewport center in tile coordinates:
      ```rust
      let (center_x, center_y) = if visible.is_empty() {
          (0.0, 0.0)
      } else {
          let min_x = visible.iter().map(|t| t.x).min().unwrap();
          let max_x = visible.iter().map(|t| t.x).max().unwrap();
          let min_y = visible.iter().map(|t| t.y).min().unwrap();
          let max_y = visible.iter().map(|t| t.y).max().unwrap();
          ((min_x + max_x) as f64 / 2.0, (min_y + max_y) as f64 / 2.0)
      };
      ```
    - Sort by manhattan distance:
      ```rust
      visible.sort_by(|a, b| {
          let dist_a = (a.x as f64 - center_x).abs() + (a.y as f64 - center_y).abs();
          let dist_b = (b.x as f64 - center_x).abs() + (b.y as f64 - center_y).abs();
          dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
      });
      ```
    - Insert sorting logic AFTER `compute_visible_tiles` call and BEFORE the `for coord in &visible` scheduling loop
    - Do NOT modify `compute_visible_tiles` (keep concerns separated — it remains a pure coordinate computation)
    - Do NOT modify `Scheduler` or `SegQueue` internals (FIFO within bucket is correct; we control enqueue order)
    - Do NOT change prefetch ring ordering (lower priority, not perceptually important)
    - _Bug_Condition: isBugCondition(input) where visible.len() > 1 in same priority bucket_
    - _Expected_Behavior: within each priority bucket, dequeue order is non-decreasing manhattan distance from viewport center_
    - _Preservation: compute_visible_tiles set membership, classify_priority logic, cross-bucket priority ordering unchanged_
    - _Requirements: 2.2, 2.3_

  - [x] 3.3 Extract center-out sort as a testable pure function
    - Create a public `sort_tiles_center_out(tiles: &mut Vec<TileCoord>)` function in `viewport.rs` (or a new `ordering.rs` module) so the sorting logic can be unit-tested independently without Tauri app state
    - This function computes the center from the tile bounding box and sorts in-place by manhattan distance
    - Call this new function from `set_viewport` instead of inline sort logic
    - Add unit tests for the extracted function covering:
      - Empty input (no-op)
      - Single tile (no-op)
      - 2×2 grid (all equidistant, stable sort)
      - 3×3 grid (center tile first, corners last)
      - 4×4 grid (proper center-out spiral)
      - Asymmetric grids (e.g., 5×2)
    - _Requirements: 2.2_

  - [x] 3.4 Handle edge cases
    - **Empty batch**: `request-tiles` with `tiles: []` — `Promise.all([])` resolves immediately, no messages posted (same as before since loop body never executes)
    - **Single tile in batch**: `request-tiles` with 1 tile — `Promise.all` with single promise is functionally equivalent to `await` on that single promise
    - **Single visible tile in viewport**: sort of 1-element array is a no-op (same result as unsorted)
    - **Equal manhattan distances**: tiles at same distance maintain stable insertion order via `sort_by` (Rust's sort is stable)
    - **Very large batches**: `Promise.all` with 100+ tiles — verify no resource exhaustion (tile:// protocol is local IPC, not real network; concurrent requests are safe)
    - Add edge-case unit tests for these scenarios in both frontend and backend test files
    - _Requirements: 2.1, 2.2_

  - [x] 3.5 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - Parallel Fetch & Center-Out Order
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 encodes the expected behavior (parallel fetches, center-out dequeue)
    - When this test passes, it confirms the expected behavior is satisfied
    - Run bug condition exploration test from step 1
    - **EXPECTED OUTCOME**: Test PASSES (confirms bug is fixed)
    - _Requirements: 2.1, 2.2, 2.3_

  - [x] 3.6 Verify preservation tests still pass
    - **Property 2: Preservation** - Single-Tile Fetch & Cross-Bucket Priority Ordering
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation property tests from step 2
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions)
    - Confirm all tests still pass after fix (no regressions)
    - Verify: single `fetch-tile` messages still produce correct responses
    - Verify: cross-bucket priority ordering still holds
    - Verify: `compute_visible_tiles` still returns same tile set (only order changed)
    - Verify: `classify_priority` still classifies identically
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6_

- [x] 4. Checkpoint - Ensure all tests pass
  - Run full frontend test suite: `cd frontend && npm test`
  - Run full Rust test suite: `cargo test --workspace`
  - Run proptest specifically: `cargo test --package engine-tiles --test bug_condition_test --test preservation_test`
  - Run vitest specifically: `cd frontend && npx vitest run src/workers/__tests__/`
  - Verify no regressions in existing `crates/engine-tiles/tests/integration_test.rs`
  - Verify no regressions in `crates/engine-project/tests/` integration tests
  - Ensure all tests pass, ask the user if questions arise.

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1", "2"] },
    { "id": 1, "tasks": ["3.1", "3.2", "3.3", "3.4"] },
    { "id": 2, "tasks": ["3.5", "3.6"] },
    { "id": 3, "tasks": ["4"] }
  ]
}
```

- Wave 0: Tasks 1 and 2 can be done in parallel (both run on UNFIXED code)
- Wave 1: Tasks 3.1–3.4 depend on wave 0 being complete (implementation)
- Wave 2: Tasks 3.5, 3.6 depend on wave 1 (verify fix and preservation)
- Wave 3: Task 4 depends on all prior tasks (final checkpoint)

## Notes

- Frontend tests use `vitest` + `fast-check` (already in dev-dependencies)
- Backend tests use `proptest` (needs to be added to `crates/engine-tiles/Cargo.toml` dev-dependencies)
- The `tile://` protocol is local Tauri IPC — unlimited concurrency is safe (no network throttling)
- `sort_by` in Rust is stable, preserving insertion order for equal-distance tiles
- `Promise.all` does not short-circuit on rejection here because `fetchAndDecodeTile` catches all errors internally and always resolves
