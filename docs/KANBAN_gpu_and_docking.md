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

### Ready — брать в работу сейчас

| Карточка | Размер | Описание |
|---|---|---|
| **A1. Decision function per-tile** | S | Заменить глобальный `gpu_preview_enabled()` на per-tile выбор: тёплый+eligible слот → GPU, иначе CPU. Использует существующий slot state, новой архитектуры не требует. `GPU_AUTO_DISPATCH_TZ.md` Фаза 1. |
| **A2. Speculative warm-up (фоновый прогрев)** | M | Low-priority фоновый promote тайлов вокруг viewport и после открытия документа. Не блокирует основной scheduling, не крадёт VRAM у активного viewport. Фаза 2. |
| **A3. Benchmark auto-dispatch vs opt-in vs CPU** | S–M | Та же дисциплина: release, n≥20, median/σ/p95/p99. Новый обязательный сценарий "открытие → pan" (cold→warm). Gate: auto-dispatch нигде не хуже CPU. Фаза 3. |
| **A4. Убрать UI-переключатель GPU** | S | Только после A3 прошёл gate. `DITHER_FORCE_CPU` остаётся debug-флагом, не продуктовой опцией. Фаза 4. |

**Зависимости внутри Ready:** A1 → A2 → A3 → A4, строго последовательно.
Не распараллеливать — A2 без A1 бессмысленен (нечего "тепло" использовать
без decision function), A4 без A3 — это ровно та ошибка, которую уже
разбирали (снять чекбокс без доказательств).

### Needs Spec — сначала решение, потом карточки

| Карточка | Что нужно перед оценкой размера |
|---|---|
| **A5. Export на GPU** | Сначала замер: насколько CPU-export реально медленный на больших документах для пользователя. Без этого числа — не заводить карточку с оценкой, это ещё не готовая задача. |
| **A6. GPU-ярус в multi-doc бюджете — доля на документ** | Проверить: `EvictContext` для GPU-яруса уже использует active/open_docs так же, как CPU RAM ярус, или это осталось декларацией. Если работает как надо — карточки не нужно, закрыть как Done. Если нет — там и там небольшая карточка S. |
| **A7. Session palette LUT — полный resident-путь** | Отмечено как "leftover" в отчёте — bridge ещё checkpoint при готовых шейдерах. Нужно уточнить объём оставшейся работы у того, кто это писал, прежде чем оценивать. Вероятно S, но не подтверждено. |
| **A8. f16 VRAM / sparse atlas** | Не заводить карточку вообще, пока нет данных, что VRAM-бюджет реально ограничивает пользователей. Explicitly не приоритет. |

---

## Трек B — Нативный докинг

### Done (не трогать без новой причины)

| Карточка | Ссылка |
|---|---|
| B1. Аудит текущей реализации докинга | `track-b-infra/AUDIT_docking_current_implementation.md` |
| B2. ADR: модель докинга (FlexLayout выбрана) | `docs/B2_ADR_flexlayout_docking.md` |
| B2 Spike: Path A — flexlayout-react работает нативно | `track-r-docking/SPIKE_EXECUTION_LOG.md` |
| **B3. Layers на FlexLayout** — Phase 1–4 ✅ | `track-r-docking/B3_tasks.md` |

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

*(пусто — B3 закрыт, следующая задача Ready)*

### Ready — брать в работу сейчас

| Карточка | Размер | Описание |
|---|---|---|
| **B4a. Миграция Effect панели** ✅ | M | Effect на FlexLayout. As-built: [`docs/FLEXLAYOUT_DOCKING.md`](./FLEXLAYOUT_DOCKING.md) (§12–13: эволюция vs долг). |
| **B4b. Миграция ColorLab панели** ✅ | M | Color Lab на FlexLayout. Dual SoT layout снят. |
| **B4c. JS popout drag / удаление global_mouseup** ✅ | S | Path A: JS `setPosition` + in-WebView mouseup; `global_mouseup` удалён; affinity hit-test оставлен. Spec: `.cursor-spec/track-r-docking/B4c_js_popout_drag_spec.md`. |

**Зависимости:** B4a → B4b → B4c, строго последовательно.  
**B4a + B4b + B4c закрыты (код).** Manual QA: feel drag + negative-origin + Linux smoke; discoverability all-floated.

**Открытый не-блоковый долг:** пин `flexlayout-react@0.7.15` vs ADR ~0.10.x; PanelManager ещё для Preview/presets.
### Needs Spec

| Карточка | Размер | Описание |
|---|---|---|
| **B7. OS-окно для мультимонитора** | M | Опционально после B4. Явное "Open in new window" через Tauri `WebviewWindowBuilder`. Не побочный эффект drag. |

---

## Сводная канбан-доска

```
BACKLOG (Needs Spec)          READY                 IN PROGRESS   DONE
─────────────────────         ──────────────────    ───────────   ────────────────────────────────
A5 Export на GPU               A1 Decision function               GPU-resident архитектура
A6 GPU multi-doc бюджет        A2 Speculative warm-up             Industrial Gate
A7 Palette LUT resident        A3 Benchmark dispatch              ED decision (CPU forever)
A8 f16/sparse (не приоритет)   A4 Убрать GPU-тоггл               Display decision (no-go)
                                                                   pixel_size>1 decision
B7 OS-окно мультимонитор                                          B1 Audit докинга
(опционально, после B4)                                           B2 ADR + Spike (Path A ✅)
                                                                  B3 Layers на FlexLayout ✅
                                                                  B4a Effect migration ✅
                                                                  B4b ColorLab migration ✅
                                                                  B4c JS popout drag ✅
```

**Что брать в работу прямо сейчас:**
- **A1** — Decision function per-tile (S, независима)
- Параллельно: QA B4c (feel / negative-origin / Linux) + discoverability all-floated + решить bump FL 0.7.15→0.10.x

**Зависимости трека B:** B4a ✅ → B4b ✅ → B4c ✅ → (B7 опционально)
