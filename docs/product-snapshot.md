# Dither Yuki 1.0.5-beta — честный снимок продукта

> Обновлено **2026-09-30**. Движок тайлов и дизера — настоящий. Ниже — что уже
> закрыто и что всё ещё beta, без продажи «как Photoshop».
> (Ранее: снимок 0.2.0 в том же духе.)

| Документов | Потолок | Кэш RAM | IPC |
|---|---|---|---|
| N вкладок (runtime `DocumentId`) | 8192² | adaptive 25% RAM, clamp 512 MiB–4 GiB (`DITHER_RAM_BUDGET_MIB`) | `commands/` + `services/` |

> **Вердикт**  
> Студия дизера на холсте до ~4K с вкладками, FlexLayout-панелями, ASCII
> (CPU full-document), Path B GPU **на тёплом viewport**, OS-превью
> `.dyproj`/`.dyuki`, и in-app updates — работает. Не продукт: paint, ICC/print,
> video/batch, default-on GPU на холодном панорамировании, отдельное OS-окно
> панели на второй монитор (B7).

---

## Что больше не блокер

| Было | Сейчас |
|---|---|
| Один документ, `doc_id = 1` | Вкладки + session registry ([multi-doc-tabs.md](./multi-doc-tabs.md)) |
| Монолитный `commands.rs` | `src-tauri/src/commands/` + `services/` |
| Жёсткий `FilterKind` dispatch для новых эффектов | `engine-registry` + `AlgorithmId` ([HOW_TO_ADD_ALGORITHM.md](./HOW_TO_ADD_ALGORITHM.md)) |
| GPU v1 `submit_lock` / 1 MB на тайл | Path B resident + auto-dispatch A1–A4 ([gpu-as-built.md](./gpu-as-built.md)) |
| Undock = только `panel-*` | Layers / Effect / Color Lab / Preview на FlexLayout ([FLEXLAYOUT_DOCKING.md](./FLEXLAYOUT_DOCKING.md)) |
| ED через голую рекурсию соседей | `EdFrontier` wavefront |
| Нет ASCII | CPU ASCII effect + export/clipboard ([ascii-as-built.md](./ascii-as-built.md)) |
| Нет OS thumbnails | Quick Look + Windows Explorer via `dither-thumb` ([PREVIEWS.md](./PREVIEWS.md)) |
| Нет slider haptics | macOS Taptic + Preferences toggle ([haptics-as-built.md](./haptics-as-built.md)) |

---

## Что всё ещё режет продукт

| Тема | Почему |
|---|---|
| **Тайл ≈ 1.03 MB f32** | Viewport легко ест сотни MB на слой; f16 (A8) не открыт |
| **ED последователен** | Корректно, но заливает viewport с угла |
| **Холодный GPU** | ~3× медленнее CPU — cold compute opt-in, не default-on |
| **Полный invalidate слайдера** | 100 ms debounce режет IPC; ASCII/Riemersma — 350 ms |
| **Нет paint / ICC / batch** | Модель документа — структура + undo snapshot |
| **ASCII GPU** | Только CPU path; GPU stages в спеке, не в default |

---

## GPU (честно)

Не продавать «GPU acceleration» как всегда быстрее. Warm Bayer / CRT / palette —
выигрыш. Cold pan и любой ED — CPU. Export на GPU — **A5 NO-GO**.

Env: `DITHER_GPU_PREVIEW=1` (cold compute), `DITHER_FORCE_CPU=1`,
`DITHER_GPU_WARMUP=0`. UI-тоггла нет.

---

## Если чинить дальше

* Track C — адаптивный RAM/VRAM (Phase 1 в дереве); диагностика T0 на реальном 8K
* A8 f16/sparse — только если occupancy реально упирается (~19% на 1080p + A2)
* B4c QA + pin/bump `flexlayout-react`
* B7 — отдельное OS-окно, не побочный эффект drag
* ASCII GPU + `CacheStage::Ascii` по evidence
* ICC / bit depth или явный sRGB-only
