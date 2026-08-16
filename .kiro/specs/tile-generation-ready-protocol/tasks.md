# Implementation Plan

Жёсткий порядок. Не начинать A, пока E+C+п.6 зелёные. Не начинать D/F в этом плане.

## Tasks

- [x] 1. Protocol Ready (E) — tests first
  - Unit/integration around `handle_tile_request` (extract Ready predicate if needed so it is testable without full Tauri):
    - `!dirty && generation >= doc_gen` → would_serve_200
    - `dirty` → 202
    - `generation < doc_gen` even if `!dirty` → 202
  - Implement check + `X-Tile-Generation` on 200
  - On 202 path, `enqueue_dedup` Immediate at **live** Doc_Gen
  - _Requirements: 1.1–1.5_

- [x] 2. Client rev on decoded + drop stale (C)
  - `tile-decoded` includes `rev`
  - TileCanvas: `rev < tileRevRef.current` → close bitmap, no pending/map
  - On rev bump: close pending bitmaps that are stale
  - Vitest: helper or message handler test — stale rev never enters displayed set
  - _Requirements: 2.1–2.5_

- [x] 3. TICKET-1 closing patch (п.6)
  - After `insert_fresh_gen == false`, if live Doc_Gen > cached generation: `mark_dirty` + enqueue current gen
  - If live == cached: leave clean
  - Extend `insert_fresh_gen_keeps_newer_generation` / new test: reject then live gen 3 → dirty + scheduled
  - Note in `TICKETS_preview_latency.md` TICKET-1: addendum acceptance, type Bug/regression
  - _Requirements: 3.1–3.4_

- [x] 4. Batch commit timeout (A) — only after 1–3
  - `COMMIT_WAIT_MS = 100`
  - Timer from first current-rev pending; commit current-rev pending; keep old bitmap for keys still missing
  - Still drop stale rev
  - Tests in `pyramidLevel.test.ts` / TileCanvas helpers
  - _Requirements: 4.1–4.5_

- [ ] 5. Measurement gate for D (no code unless data)
  - After 1–4 in a release build: FS + pixel_size 8, large doc, slider drag
  - Log/count: 202 exhausted (5 retries) vs recovered via tile-ready vs stale-200 (should be ~0 after E)
  - Write numbers under TICKET-10; open D only if exhausted is common **and** cancel-on-rev is designed
  - _Requirements: 5.1–5.3_

- [x] 6. Explicit skip
  - Do not implement B, G, F
  - _Requirements: 6.1–6.4_
