# Тайловый Pipeline: применение эффектов и координатная система

> Техническая документация по тайловой обработке, глобальным координатам,
> cross-tile зависимостям и порядку вычислений.

---

## 1. Обзор тайловой системы

Холст разбит на тайлы 256×256 пикселей. Каждый тайл хранится как `PixelTile` —
массив (260)² × 4 каналов f32 (RGBA linear). Дополнительные 2 пикселя с каждой
стороны (HALO = 2) обеспечивают контекст для фильтров с error diffusion.

### Стадии (CacheStage)

```
Raw → [filter stack] → Processed → [compositor] → Composite
```

| Стадия | Описание |
|--------|---------|
| Raw | Исходные пиксели слоя (из decode/import) |
| Processed | После применения filter stack слоя |
| Composite | После blending всех видимых слоёв (финальный результат для отображения) |

---

## 2. Глобальные координаты (GlobalCoord)

### Проблема

Фильтры с периодическим паттерном (Bayer, CRT, Wave) должны давать непрерывный
результат на стыках тайлов. Если координаты для threshold-матрицы считаются
локально от тайла, паттерн перезапускается на каждой границе — виден шов.

### Решение: `engine-tiles/src/coords.rs`

```rust
/// Для core area (local_x ∈ [0, TILE_SIZE))
GlobalCoord::from_local(tile_coord, local_x, local_y)
    → { x: tile.x * 256 + local_x, y: tile.y * 256 + local_y }

/// Для полного тайла с halo (local_x ∈ [0, TILE_SIZE + 2*HALO))
GlobalCoord::from_tile_pixel(tile_coord, pixel_x, pixel_y)
    → { x: tile.x * 256 + pixel_x, y: tile.y * 256 + pixel_y }

/// Signed версия для halo-региона (может быть отрицательной)
GlobalCoordSigned::from_local_with_halo(tile_coord, local_x, local_y, halo)
    → { x: tile.x * 256 + local_x - halo, y: tile.y * 256 + local_y - halo }
```

### Критические правила

1. **Всегда `rem_euclid`** для индексации в паттерн: `-1 % 8 = -1` (Rust remainder),
   но `(-1i32).rem_euclid(8) = 7` (true modulo). Все threshold lookups используют
   `rem_euclid` через `pattern_cell()`.

2. **Всегда `div_euclid`** для pixel_size alignment: `(-1) / 4 = 0` (truncates toward zero),
   но `(-1i32).div_euclid(4) = -1` → `-1 * 4 = -4` (correct floor). Все блочные
   выравнивания используют `aligned()`.

3. **Не дублировать формулу.** Все новые фильтры обязаны использовать `GlobalCoord`
   или `GlobalCoordSigned`, а не считать `tile.x * 256 + local_x` инлайн.

---

## 3. Типы фильтров и их зависимости

### 3.1 Независимые от порядка (per-tile, параллельные)

| Фильтр | Зависимость между тайлами | Файл |
|--------|--------------------------|------|
| Curves | Нет | `filters/curves.rs` |
| Levels | Нет | `filters/levels.rs` |
| Ordered Dither (Bayer/CustomPng/Wave) | Нет (координаты глобальные) | `filters/dither_ordered.rs` |
| CMYK Halftone | Нет (`GlobalCoord` + screen angles) | `filters/dither_ordered.rs` |
| CRT scanlines | Нет (`Y_g` / `X_g` via `GlobalCoord`) | `filters/crt.rs` |
| Glow (blur ≤ HALO) | Нет (v1 radius capped to HALO) | `filters/glow.rs` |
| PaletteQuantize (без diffusion) | Нет | `filters/palette_quantize.rs` |
| Glitch | Нет (seed deterministic) | `filters/glitch.rs` |

Эти фильтры могут обрабатывать любой тайл в любом порядке и параллельно.
Pattern filters (Bayer, CMYK Halftone, Wave, CRT) MUST obtain document coords only
via `GlobalCoord` / `GlobalCoordSigned` — never `tile.y * 256 + local_y` inline.
Seam tests: `phase1_pattern_seam.rs`, `crt.rs` unit tests, `ordered_dither_seamless_*`.

SVG export (document composite → greedy meshing / contour paths) lives in
`engine-io::svg_export` and does not participate in the tile pipeline.

### 3.2 Зависимые от порядка (requires_full_row = true)

| Фильтр | Зависимость | Файл |
|--------|------------|------|
| Error Diffusion (Floyd-Steinberg) | Left + Top neighbor | `filters/dither_diffusion.rs` |
| Error Diffusion (Atkinson) | Left + Top neighbor | `filters/dither_diffusion.rs` |

Эти фильтры распространяют ошибку квантизации на соседние пиксели. На границе
тайла ошибка "перетекает" в соседний тайл через `ErrorResidualsStore`.

**Зависимость:** тайл (X, Y) нуждается в residuals от:
- (X-1, Y) — правый край соседа слева → первые 2 колонки текущего тайла
- (X, Y-1) — нижний край соседа сверху → первые 2 строки текущего тайла

---

## 4. Cross-Tile Error Diffusion Pipeline

### 4.1 ErrorResidualsStore

```rust
pub struct ErrorResidualsStore {
    entries: DashMap<(LayerId, TileCoord), ErrorResiduals>,
}

pub struct ErrorResiduals {
    pub right: Vec<f32>,   // TILE_SIZE rows × 2 cols × 3 channels
    pub bottom: Vec<f32>,  // 2 rows × TILE_SIZE cols × 3 channels
    pub corner: Vec<f32>,  // CORNER_PATCH×CORNER_PATCH×3 → tile (tx+1, ty+1)
}
```

- После обработки тайла (X, Y): store residuals под ключом `(layer, {X, Y})`
- Перед обработкой (X+1, Y): read `get_left(layer, {X+1, Y})` → seeding error buffer
- Перед обработкой (X, Y+1): read `get_top(layer, {X, Y+1})` → seeding error buffer
- Перед обработкой (X+1, Y+1): read `get_diag(layer, {X+1, Y+1})` → seed `corner`
  (IncomingErrorBuffer — диагональный FS/Atkinson overflow больше не отбрасывается)

### 4.2 Row-Major Dependency Enforcement

Тайлы обрабатываются worker pool в произвольном порядке (scheduler dequeue by priority).
Для error diffusion это создаёт проблему: если (1,0) обработан раньше (0,0),
residuals от (0,0) ещё не существуют → шов на границе.

**Решение (on-demand recursive):** `compute_processed_tile` проверяет:

```
if layer.has_error_diffusion:   # all pyramid levels
    if left_neighbor needs recompute (missing OR dirty) and raw present:
        recursively compute left_neighbor first
    if top_neighbor needs recompute …:
        recursively compute top_neighbor first
    if diag (x-1,y-1) needs recompute …:
        recursively compute diagonal neighbor first
    if neighbor raw missing:
        increment diffusion_skip_counter; register pending_diffusion_waiters;
        proceed with zero-seed this pass (wake on later raw insert)
```

Рекурсия:
- На **всех** pyramid levels (ключи residuals включают полный `TileCoord.level`)
- Только если raw tile соседа существует; иначе silent-skip + waiter registration
- `is_dependency = true` → если тайл свежий в кэше, сразу возвращаем (без recompute)

**Diagnosis (Track A §1):** raw level-0 обычно присутствует после `decompose`
(eviction в production path сейчас не вызывается) — ветка skip редко достижима;
контракт waiters зафиксирован хелперами + unit-тестами и лёгкой prod-проводкой.

### 4.3 Invalidation Flow

При изменении параметров DitherV2 фильтра:

```
1. error_residuals.clear()           — сброс ВСЕХ residuals
2. invalidate(LayerFilterChanged)    — mark dirty: Processed + Composite тайлы
3. schedule_dirty_viewport_tiles()   — enqueue задачи в scheduler
4. Worker dequeue → compute_processed_tile:
   a. Detect requires_full_row
   b. Check left/top neighbors: dirty? → recompute them first
   c. Neighbors produce fresh residuals → store
   d. Current tile reads fresh residuals → seamless result
```

---

## 5. Ordered Dithering (dither_ordered.rs)

### Координатный поток

```rust
for y in 0..TILE_FULL_SIZE {        // 0..260 (including halo)
    for x in 0..TILE_FULL_SIZE {
        // Глобальная координата с учётом halo offset:
        let gx = coord.x as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
        let gy = coord.y as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;

        // Pixel-size alignment (div_euclid для корректной работы с отрицательными):
        let block_gx = gx.div_euclid(pixel_size) * pixel_size;
        let block_gy = gy.div_euclid(pixel_size) * pixel_size;

        // Threshold lookup (rem_euclid для корректного модуло):
        let threshold = get_threshold_i32(mode, block_gx, block_gy, cache);
        // → внутри: (gx as i64).rem_euclid(matrix_size) → valid [0, matrix_size)
    }
}
```

**Гарантия бесшовности:** глобальные координаты непрерывны между тайлами,
`rem_euclid` даёт корректные позитивные индексы для любых координат (включая
отрицательные в halo-регионе). Тест: `ordered_dither_seamless_across_tile_boundary`.

---

## 6. Error Diffusion (dither_diffusion.rs)

### Алгоритм (для одного тайла)

```
1. Seed error_buf от left/top/diag neighbors (ErrorResidualsStore)
2. For each pixel (x, y) в core area [0..TILE_SIZE), L→R, T→B:
   a. Read source pixel + accumulated error
   b. Quantize (uniform or palette via Oklab KD-tree)
   c. Compute quantization error
   d. Distribute error to neighbors (FS/Atkinson kernel)
   e. Overflow → right / bottom / corner (IncomingErrorBuffer) buffers
3. Store ErrorResiduals { right, bottom, corner }
4. Copy halo region unchanged from input
```

### Pixel-size blocking (pixel_size > 1)

Для mega-pixel grid: блок pixel_size × pixel_size получает один цвет.
- Block representative: `(gx / ps) * ps` — top-left pixel блока
- Only representative participates in error diffusion
- Non-representatives copy color from representative
- Global coordinate alignment через `coord.x * TILE_SIZE + tile_x`

---

## 7. Compositor (engine-project/compositor.rs)

### Алгоритм

```
1. Start with transparent tile (all zeros)
2. Walk layer tree bottom-to-top:
   a. Leaf → fetch Processed tile → apply mask → blend into composite
   b. Group → push fresh tile → recurse children → blend group result
3. Return composite tile
```

### Blend modes

12 режимов: Normal, Multiply, Screen, Overlay, Darken, Lighten,
ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion.

Все реализованы per-pixel с SIMD-ускорением (wide f32x4) для Porter-Duff "over".

---

## 8. Scheduler и Worker Pool

### Приоритеты

| Priority | Когда | Источник |
|----------|-------|---------|
| Immediate | tile:// cache miss (202 response) | tile protocol handler |
| ViewportCenter | Центральные visible тайлы | set_viewport |
| ViewportEdge | Крайние visible тайлы | set_viewport |
| Prefetch | 1-tile ring за viewport | set_viewport |

### Worker Loop

```
loop {
    task = scheduler.dequeue()   // highest priority first
    if task.generation != current_gen → discard (stale)
    match task.stage:
        Raw → load_raw_tile
        Processed → compute_processed_tile  // + dependency enforcement
        Composite → compute_composite_tile  // + ensure_processed_tiles_fresh
    insert_fresh into cache
    emit tile-ready event → frontend
}
```

### Staleness Check

Задачи носят `generation` (document gen) и `layer_generation` (layer gen) от момента
создания. Если document или layer gen продвинулся — задача стала stale, worker
её пропускает. Это предотвращает перезапись свежих результатов устаревшими.

---

## 9. Правила для разработки новых фильтров

1. **Координаты:** использовать `GlobalCoord` / `GlobalCoordSigned` из `engine_tiles::coords`.
   Не считать `tile.x * 256 + local_x` вручную.

2. **Периодические паттерны:** вызывать `.pattern_cell(size)` для индексации в матрицу.
   Метод гарантирует корректный результат для отрицательных координат (halo region).

3. **Pixel-size alignment:** вызывать `.aligned(pixel_size)` безусловно (для pixel_size=1
   это no-op). Не оборачивать в `if pixel_size > 1`.

4. **Cross-tile зависимость:** если фильтр распространяет данные между соседними
   пикселями (error diffusion, blur, etc.):
   - Установить `requires_full_row = true` в `FilterInstance::new()`
   - Использовать `ErrorResidualsStore` или аналогичный механизм для передачи данных
   - Pipeline автоматически обеспечит row-major порядок обработки

5. **Halo:** использовать `HALO`-регион (2px border) для чтения контекста за границей
   тайла. Halo копируется из соседних тайлов при decompose и из input при filter apply.

6. **Determinism:** все фильтры должны быть детерминированы при одинаковых входных данных
   (одинаковый input tile + coord + params → одинаковый output). Для Glitch — seed-based PRNG.

---

## 10. GPU path (`engine-gpu`, Track D)

Optional wgpu compute for **pattern** filters. Error Diffusion (FS/Atkinson) stays CPU-only.

### Switches

| Env | Effect |
|-----|--------|
| `DITHER_FORCE_CPU=1` | Never use GPU (skip adapter init in app, or force CPU in apply) |
| `DITHER_GPU=1` | Prefer GPU when `GpuContext` is available (default: CPU until ops flip) |

### Contract

- **I/O:** RGBA32 float core `256×256` only (no halo in v1 GPU path).
- **Uniforms:** `tile_offset = (tile.x * 256, tile.y * 256)` — same as `GlobalCoord::from_local(tile, 0, 0)`.
- **Workgroup:** `16×16`; dispatch `(16, 16, 1)`.
- **Eligible:** Bayer2/4/8 (`pixel_size==1`, no palette); CMYK Halftone (same + RGB); CRT.
- **Not eligible:** ED, CustomPng, Wave (v1), Glow (deferred CPU), `pixel_size>1`, palette (CPU post-pass later).
- **Fallback:** map_async timeout/error → increment `GpuContext::map_timeout_counter` → CPU path.
- **Submit sync:** mutex on `GpuContext` for encode/submit/map (worker-safe).
- **Parity:** Bayer exact (`f32 ==`); Halftone/CRT max ‖Δ‖∞ ≤ `1/255`.

See `.cursor-spec/track-d-gpu/` for design and tasks.
