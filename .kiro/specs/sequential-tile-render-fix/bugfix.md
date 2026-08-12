# Bugfix Requirements Document

## Introduction

Tiles in the viewport render sequentially in raster order (left-to-right, top-to-bottom) instead of appearing simultaneously from the viewport center outward. This creates a noticeable "scan-line" loading pattern where the user waits for all preceding tiles before center tiles appear, degrading perceived performance. The root cause is twofold: (1) the frontend Web Worker awaits each tile fetch sequentially in a loop, serializing all requests, and (2) the backend enqueues tiles within a priority bucket in nested-loop enumeration order (FIFO) rather than center-out distance order.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN the frontend receives a `request-tiles` batch THEN the system fetches tiles one-by-one using sequential `await` in a for-loop, serializing all network requests so each tile must complete before the next begins

1.2 WHEN the backend enqueues visible tiles in `set_viewport` THEN the system iterates tiles in row-major raster order (top-to-bottom, left-to-right from `compute_visible_tiles`) and pushes them into the SegQueue in that order, causing FIFO dequeue within the same priority bucket to produce raster-order processing instead of center-out

1.3 WHEN both defects combine THEN the system renders tiles in a sequential scan-line pattern from top-left to bottom-right, making center-of-viewport content appear last rather than first

### Expected Behavior (Correct)

2.1 WHEN the frontend receives a `request-tiles` batch THEN the system SHALL issue all tile fetch requests in parallel (via `Promise.all` or equivalent) and post each decoded bitmap back to the main thread as soon as it resolves, independently of other tiles in the batch

2.2 WHEN the backend enqueues visible tiles in `set_viewport` THEN the system SHALL sort tiles within each priority bucket by ascending manhattan distance from the viewport center before enqueueing, so that center tiles are dequeued and processed first

2.3 WHEN both fixes are applied THEN the system SHALL render tiles appearing from the viewport center outward, with all visible tiles loading concurrently rather than sequentially

### Unchanged Behavior (Regression Prevention)

3.1 WHEN tiles are fetched via the `tile://` protocol THEN the system SHALL CONTINUE TO decode RGBA8 bytes into ImageBitmap and transfer them to the main thread with the same message format (`tile-decoded`, `tile-pending`, `tile-error`)

3.2 WHEN a tile fetch returns HTTP 202 (pending) THEN the system SHALL CONTINUE TO post a `tile-pending` message so the main thread can re-request on `tile-ready` event

3.3 WHEN a tile fetch fails with a network error or non-200/202 status THEN the system SHALL CONTINUE TO post a `tile-error` message with the error details

3.4 WHEN the scheduler dequeues tasks THEN the system SHALL CONTINUE TO respect cross-bucket priority ordering (Immediate > ViewportCenter > ViewportEdge > Prefetch)

3.5 WHEN `set_viewport` is called THEN the system SHALL CONTINUE TO classify tiles into ViewportCenter and ViewportEdge priorities based on distance from viewport center, and prefetch ring tiles as Prefetch priority

3.6 WHEN a single tile is requested via `fetch-tile` message THEN the system SHALL CONTINUE TO fetch and decode that individual tile immediately without batching

---

## Bug Condition (Formal)

```pascal
FUNCTION isBugCondition(X)
  INPUT: X of type TileBatchRequest
  OUTPUT: boolean

  // The bug manifests whenever multiple tiles are requested in a single batch
  RETURN X.tiles.length > 1
END FUNCTION
```

### Property: Fix Checking — Parallel Fetch

```pascal
// Property: All tiles in a batch are fetched concurrently
FOR ALL X WHERE isBugCondition(X) DO
  startTimes ← recordFetchStartTimes(X.tiles)
  // All fetches must be initiated within a negligible time window (not sequentially)
  ASSERT max(startTimes) - min(startTimes) < EPSILON
END FOR
```

### Property: Fix Checking — Center-Out Ordering

```pascal
// Property: Within a priority bucket, tiles closer to viewport center are dequeued first
FOR ALL X WHERE isBugCondition(X) DO
  center ← computeViewportCenter(X.viewport)
  dequeueOrder ← recordDequeueOrder(scheduler, X.priorityBucket)
  FOR i FROM 0 TO dequeueOrder.length - 2 DO
    dist_i ← manhattanDistance(dequeueOrder[i], center)
    dist_next ← manhattanDistance(dequeueOrder[i+1], center)
    ASSERT dist_i <= dist_next
  END FOR
END FOR
```

### Preservation Goal

```pascal
// Property: Preservation Checking — non-batch single-tile requests unchanged
FOR ALL X WHERE NOT isBugCondition(X) DO
  ASSERT F(X) = F'(X)
END FOR
```
