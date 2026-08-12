# TileCanvas ↔ Worker Boundary

## React side (`TileCanvas.tsx`)
- **Props in:** `docId`, `docWidth`, `docHeight`, `viewport`, `onViewportChange`
- **Owns:** canvas DOM, visible-tile computation, `tile-ready` event subscription, posting decode jobs to the worker, blit of decoded bitmaps
- **Does not own:** document/layer/filter state, IPC mutate commands, zoom UI chrome

## Worker side (`workers/tileWorker.ts`)
- **Messages in:** tile decode / process jobs (binary payloads from engine)
- **Messages out:** transferable ImageBitmap / pixel buffers for blit
- **Out of P3 scope:** do not refactor worker internals; only keep this contract stable

## Integration
- `PreviewWindow` / `PreviewFeature` compose TileCanvas with local viewport camera
- Engine schedules tiles via `set_viewport` IPC; canvas reacts to `tile-ready` events
