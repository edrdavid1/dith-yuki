# Канбан: GPU и Докинг — два трека

Формат: карточки по трекам, с колонкой готовности. `Done` — уже закрыто
в прошлых сессиях, не трогать без новой причины. `Ready` — можно брать в
работу прямо сейчас, спека есть. `Needs Spec` — сначала нужно решение/
исследование, потом получится карточка размером в спринт, не раньше.

Размер: S (дни), M (неделя-две), L (месяц+, почти всегда = отдельный
под-трек с собственным ADR).

---

## Трек A — Взрослый GPU

### Done (не трогать без новой причины)

| Карточка | Ссылка |
|---|---|
| GPU-resident архитектура (cache, graph, executor) | `gpu-path-b/SPEC.md`, `TASKS.md` |
| Bayer/Halftone/CRT/Palette breadth + multi-layer composite | `gpu-path-b/TASKS.md` T7, T7.5 |
| Industrial Gate: честная оценка, R1 = opt-in | `gpu-industrial-gate/REPORT.md` |
| ED на GPU — исследовано, решение "навсегда CPU" | `ED_DECISION.md` |
| Нативный display surface — решение "no-go" | `DISPLAY_DECISION.md` |
| `pixel_size > 1` на GPU — решение "Non-goal" | Industrial Gate H2 |
| v1 per-tile GPU удалён | `TASKS.md` T9 |
| **A1. Decision function per-tile** | `.cursor-spec/gpu-auto-dispatch/` |
| **A2. Speculative warm-up** | `gpu-auto-dispatch/`, фаза 2 |
| **A3. Benchmark auto-dispatch vs opt-in vs CPU** | `gpu-auto-dispatch/A3_EVIDENCE.md` (Apple M3 n=20 PASS) |
| **A4. Убрать UI-переключатель GPU** | Preferences GPU section removed |
| **A5. Export на GPU** | **NO-GO** — `gpu-export-a5/` |
| **A6. GPU-ярус в multi-doc бюджете** | Same `EvictContext` as CPU RAM |
| **A7. Session palette LUT на resident** | Bridge: Guided/Mixed/PaletteQuantize (no ED) via `PaletteLutCache`; Strict/Simple remain CPU |

### Ready — брать в работу сейчас

Трек A: A1–A7 закрыты (A5 NO-GO). Остался A8 (не приоритет).

**Зависимости (закрыты):** A1 ✅ → A2 ✅ → A3 ✅ → A4 ✅.

### Needs Spec — сначала решение, потом карточки

| Карточка | Что нужно перед оценкой размера |
|---|---|
| **A8. f16 VRAM / sparse atlas** | Occupancy harness: `gpu-vram-a8/`. Preview+A2 ~19% on 3072²/1080p. Full-doc GPU fill saturates (expected). **f16 not started** — need real-file preview peak. |

---

## Трек B — Нативный докинг

### Done (не трогать без новой причины)

| Карточка | Ссылка |
|---|---|
| B1. Аудит текущей реализации докинга | `track-b-infra/AUDIT_docking_current_implementation.md` |
| B2. ADR: модель докинга (FlexLayout выбрана) | `docs/B2_ADR_flexlayout_docking.md` |
| B2 Spike: Path A — flexlayout-react работает нативно | `track-r-docking/SPIKE_EXECUTION_LOG.md` |
| **B3. Layers на FlexLayout** — Phase 1–4 ✅ | `track-r-docking/B3_tasks.md` |
| **B4a. Effect на FlexLayout** ✅ | [`FLEXLAYOUT_DOCKING.md`](./FLEXLAYOUT_DOCKING.md) |
| **B4b. Color Lab на FlexLayout** ✅ | same; dual SoT layout снят |
| **B4c. JS popout drag** ✅ | `global_mouseup` удалён; spec `B4c_js_popout_drag_spec.md` |

**B3 итог (2026-08-31):**
- ✅ `flexlayout-react` 0.7.15 установлен
- ✅ `FlexLayoutContainer` с реальным `<Layout>`, debounced save (500 ms)
- ✅ `layoutPanelFactory`: layers→LayersFeature, effect/colorlab→B4 placeholder
- ✅ `LayoutProvider` в providers.tsx: `Model.fromJson()` при старте
- ✅ `AppLayout`: layers убран из DockedSidebar, рендерится через FlexLayoutContainer
- ✅ Rust persistence (16 тестов, >90% покрытие), 3 Tauri команды
- ✅ `cargo build` 0 ошибок, `npx tsc --noEmit` 0 ошибок
- ✅ Старый PanelManager (effect/colorlab) не тронут — B4

### In Progress — текущая работа

*(пусто)*

### Ready — брать в работу сейчас

| Карточка | Размер | Описание |
|---|---|---|
| **B4c manual QA** | S | Feel JS drag vs old OS-drag; negative-origin; Linux smoke; all-floated discoverability |
| **FL version pin** | S | Явно зафиксировать `0.7.15` или bump → 0.10.x (перепрогнать popout-патч) |

**B4a + B4b + B4c закрыты (код).**  
**Открытый не-блоковый долг:** PanelManager ещё для Preview/Preferences.

### Needs Spec

| Карточка | Размер | Описание |
|---|---|---|
| **B7. OS-окно для мультимонитора** | M | Опционально после B4. Явное "Open in new window" через Tauri `WebviewWindowBuilder`. Не побочный эффект drag. |

---

## Сводная канбан-доска

```
BACKLOG (Needs Spec)          READY                 IN PROGRESS   DONE
─────────────────────         ──────────────────    ───────────   ────────────────────────────────
A8 f16/sparse (не приоритет)                                     Industrial Gate
                                                                   ED decision (CPU forever)
                                                                   Display decision (no-go)
                                                                   pixel_size>1 decision
                                                                   **A1–A4 auto-dispatch** ✅
                                                                   **A5 GPU export NO-GO**
                                                                   **A6 GPU EvictContext** ✅
                                                                   **A7 palette LUT bridge** ✅
B7 OS-окно мультимонитор                                          B1 Audit докинга
(опционально, после B4)                                           B2 ADR + Spike (Path A ✅)
                                                                  B3 Layers на FlexLayout ✅
                                                                  B4a Effect migration ✅
                                                                  B4b ColorLab migration ✅
                                                                  B4c JS popout drag ✅
```

**Что брать в работу прямо сейчас:**
- Трек A: остался только A8 (не приоритет).
- Параллельно: QA B4c (feel / negative-origin / Linux) + discoverability all-floated + решить bump FL 0.7.15→0.10.x

**Зависимости трека B:** B4a ✅ → B4b ✅ → B4c ✅ → (B7 опционально)
