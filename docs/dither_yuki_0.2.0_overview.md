# Dither Yuki 0.2.0 — честный снимок продукта

> Обновлено **2026-09-04**. Движок тайлов и дизера — настоящий. Ниже — что уже закрыто и что всё ещё beta, без продажи «как Photoshop».

| Документов | Потолок | Кэш RAM | IPC |
|---|---|---|---|
| N вкладок (runtime `DocumentId`) | 8192² | process 512 MiB + Raw pin | `commands/` + `services/` |

> **Вердикт**  
> Студия дизера на холсте до ~4K с вкладками, FlexLayout-панелями и Path B GPU **на тёплом viewport** — работает. Не продукт: paint, ICC/print, video/batch, default-on GPU на холодном панорамировании, OS-окно на второй монитор (B7).

---

## Что больше не блокер

| Было (август) | Сейчас |
|---|---|
| Один документ, `doc_id = 1` | Вкладки + session registry ([multi-doc-tabs.md](./multi-doc-tabs.md)) |
| `commands.rs` ~5000 строк | Разнесено (`src-tauri/src/commands/` + `services/`) |
| GPU v1 `submit_lock` / 1 MB на тайл | Path B resident + auto-dispatch A1–A4 ([gpu-as-built.md](./gpu-as-built.md)) |
| Undock = только `panel-*` + `global_mouseup` | Layers / Effect / Color Lab на FlexLayout; JS popout drag ([FLEXLAYOUT_DOCKING.md](./FLEXLAYOUT_DOCKING.md)) |
| ED через голую рекурсию соседей | `EdFrontier` wavefront (incremental residuals — follow-up) |

---

## Что всё ещё режет продукт

| Тема | Почему |
|---|---|
| **Тайл ≈ 1.03 MB f32** | Viewport легко ест сотни MB на слой; f16 (A8) не открыт |
| **ED последователен** | Корректно, но заливает viewport с угла |
| **Холодный GPU** | ~3× медленнее CPU — поэтому cold compute opt-in, не default-on |
| **Полный invalidate слайдера** | 100 ms debounce режет IPC, не работу |
| **Нет paint / ICC / batch** | Модель документа — структура + undo snapshot |
| **Preview не на FlexLayout** | PanelManager leftover; B7 OS-окно — Needs Spec |

---

## GPU (честно)

Не продавать «GPU acceleration» как всегда быстрее. Warm Bayer / CRT / palette — выигрыш. Cold pan и любой ED — CPU. Export на GPU — **A5 NO-GO**.

Env: `DITHER_GPU_PREVIEW=1` (cold compute), `DITHER_FORCE_CPU=1`, `DITHER_GPU_WARMUP=0`. UI-тоггла нет.

---

## Если чинить дальше

* A8 f16/sparse — только если preview occupancy реально упирается (сейчас ~19% на 1080p + A2)
* B4c QA + pin/bump `flexlayout-react`
* B7 — отдельное OS-окно, не побочный эффект drag
* ICC / bit depth или явный sRGB-only
* Halo-less preview composite; command-pattern undo (не приоритет)
