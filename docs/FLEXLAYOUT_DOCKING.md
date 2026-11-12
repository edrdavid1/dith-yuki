# FlexLayout: интеграция докинга

**Дата:** 2026-09-04  
**Статус:** as-built (B3 + B4a + B4b + B4c)  
**Библиотека:** [`flexlayout-react`](https://github.com/caplin/FlexLayout) **0.7.15** (MIT; см. [`legal/flexlayout-license-snapshot.md`](./legal/flexlayout-license-snapshot.md))

Этот документ — актуальная картина: как библиотека подключена, какие окна мигрированы, кто чем владеет, как устроены float/redock и persistence.

Расхождения с исходным ADR (§12) разделены на **оправданную эволюцию** и **технический долг** — не смешивать.

---

## 1. Зачем FlexLayout

Старый докинг (Rust `PanelManager` + affinity + отдельные `panel-*` окна) плохо закрывал reorder/split внутри сайдбара и усложнял popout. ADR B2 выбрал FlexLayout:

- открытая лицензия (MIT);
- JSON-модель layout из коробки;
- popout через FloatingWindow / portal;
- drag-reorder и split без своей геометрии.

**Важно:** После **B4b** все три докируемые панели на FlexLayout. После **B4c** drag-to-redock для Flex popout — JS `setPosition` + in-WebView `mouseup` (без `global_mouseup` / без OS `startDragging`). Hit-test зон остаётся в `dock_affinity` (тонкий мост). Linux больше не блокируется отсутствием mouseup-backend.
---

## 2. Что мигрировано

| Панель (component id) | Контейнер | Содержимое | Undock |
|-----------------------|-----------|------------|--------|
| `layers` | **FlexLayout** | `LayersFeature` | `flex-popout-*` + `FlexPopoutChrome` |
| `effect` | **FlexLayout** | `EffectsFeature` | то же |
| `colorlab` | **FlexLayout** | `ColorLabFeature` | то же |
| `preview` | **FlexLayout center** | Canvas preview | `flex-popout-*` + `FlexPopoutChrome` (close → canvas) |
| `preferences` / Help | диалог | — | не FL |

Признак миграции на фронте:

```ts
isPanelOnFlexLayout(id) === (id === 'layers' || id === 'effect' || id === 'colorlab' || id === 'preview')
```

Фабрика: `frontend/src/factories/layoutPanelFactory.tsx`.

Дефолт при пустом/битом layout:

- **left** → Layers  
- **right** → Effect + Color Lab (вертикальный стек)
- **center** → Preview

(`frontend/src/defaults/DefaultLayouts.ts`)

**B4b persistence:** если у пользователя уже есть `flexlayout_right.json` без `colorlab`, при загрузке таб Color Lab **добавляется** справа (не затирая Layers/Effect); toast: `LAYOUT_TOAST_MESSAGES.COLORLAB_ADDED`.

**B5:** Preview на center FlexLayout Model (`flexlayout_center.json`). Float = тот же JS popout path, что у Layers/Effect/Color Lab. Affinity для Preview не армится (close возвращает на canvas). Legacy `panel-*` / PanelManager удалены.
---

## 3. Кто чем владеет

| Забота | Владелец |
|--------|----------|
| Дерево табов, same-side drag/split, float-геометрия | FlexLayout `Model` (**два** инстанса: left + right) |
| Ширина колонки, collapse, split ratio | `ShellContext` (`dither.shellPrefs` в localStorage) |
| Preview (center) / Preferences + Help dialogs | Preview = center Flex Model; Preferences/Help = dialogs (не FL) |
| Persist JSON FlexLayout | Tauri `save_layout_{left,right,center}` / `load_layout_{left,right,center}` |
| Drag-to-redock (hit-test зон) | Rust `dock_affinity` + zone IPC; **complete** = JS mouseup in popout → `complete_float_drag` (B4c). `global_mouseup` удалён. |

Два Model — осознанно: shell уже делит экран на левую/правую колонку + canvas. Перенос панели между сторонами — не нативный drag FlexLayout, а `movePanelBetweenSides`.

**После B4c:** SoT layout — Flex Model; redock popout — JS drag (path A).  
**После B5:** Preview тоже на Flex Model (center); float = JS popout; affinity для Preview не армится (close → canvas).

```
┌──────────────┬────────────────────┬──────────────┐
│ left column  │  center / Preview  │ right column │
│              │                    │              │
│ Model L      │ Model C (preview)  │ Model R      │
│ FlexLayout   │ FlexLayout         │ FlexLayout   │
│ (Layers …)   │                    │ (Effect +    │
│              │                    │  Color Lab)  │
└──────────────┴────────────────────┴──────────────┘
         ▲               ▲                      ▲
         └── flex-popout-* JS setPosition + mouseup ──┘
              (affinity hit-test; Preview close→center)
```

Providers: `ShellProvider` → `ShortcutsProvider` → `LayoutProvider` → `AppLayout`.

---

## 4. Как устроена колонка (`AppLayout`)

W10: на каждую сторону только два режима (legacy `DockedSidebar` / Mixed удалены):

| Режим | Условие | UI |
|-------|---------|-----|
| **Docked (flex-only)** | есть docked flex-табы | `FlexLayoutContainer` + collapse / resize |
| **Float-host** | flex-табы есть, все floating | off-screen host + invisible edge drop-rail для redock (без Return-strip в main; возврат = close popout / header button / native Window menu) |

`layoutEpoch` / store version заставляет shell пересчитать ширину/host после float/dock **без** клонирования `Model` (клон ломает popout).

Collapse: `SidebarCollapseStrip` (40px); drag ширины &lt; **220px** → collapse. Emergency collapse (W8) — временный strip без записи в prefs.

---

## 5. Интеграция библиотеки (фронт)

### 5.1 Точки входа

| Файл | Роль |
|------|------|
| `contexts/LayoutContext.tsx` | два Model, load/save, move/float/dock/redock, epoch |
| `components/FlexLayoutContainer.tsx` | один `<Layout>` на сторону, titlebar, debounce save |
| `factories/layoutPanelFactory.tsx` | component id → React feature + popout wrap |
| `defaults/DefaultLayouts.ts` | дефолтный `IJsonModel` + chrome policy |
| `shared/styles/flexlayout-theme.css` | визуал под Color Lab / Chicago |
| `public/popout.html` | пустой документ для FloatingWindow |

### 5.2 Chrome policy (не стоковый UI FlexLayout)

При загрузке модели (`applyAppChromePolicy`):

- tab strip включён, single-tab stretch;
- **`tabEnableFloat: false`** — штатная кнопка float скрыта (float делаем сами через `Actions.floatTab`);
- нет rename / close / maximize на табах;
- splitter тонкий (1px + extra hit area).

Docked titlebar рисуется через `onRenderTab` → общий `WindowTitlebar` (variant docked: □ · ridges · Title · ridges · □).  
Тело панели с `hideChrome`, чтобы не было второго заголовка.

### 5.3 Стек и side-by-side (multi-column)

**Модель данных (без нового layout engine):** один FlexLayout `Model` на сторону (`left` / `right` / `center`). Внутри сайдбара:

- вертикальный стек = `column` из tabset’ов (как раньше);
- side-by-side = `row` из tabset’ов (каждая «колонка» — свой tabset со стеком);
- ширина outer chrome (`ShellContext`) — отдельный prefs JSON; ширины tabset’ов внутри — веса FlexLayout.

**Drop zones (stock FlexLayout ~25% края):**

- top/bottom → стек в той же колонке (`data-drop-axis=stack`);
- left/right → новая колонка рядом (`data-drop-axis=side`);
- `center` в `onAction` → `bottom` (WindowTitlebar не делит strip);
- 3-я колонка на стороне запрещена (`MAX_SIDE_COLUMNS = 2`); 2-я колонка принимается только если `computeShellWidths` без emergency collapse; иначе outline `data-drop-denied` и `onAction` отклоняет drop. При принятии сайдбар дорастает до ≥240×N (prefs), а display ширины всегда из solver.
- **Важно:** titlebar undock и native FlexLayout tab-drag стартуют с одного mousedown. Перед `floatPanel` вызывается `cancelFlexLayoutDrag`, а `onAction` отклоняет `MOVE_NODE` уже floating-таба — иначе mouseup с edge-dock (`enableEdgeDock`) переносит живой popout и окно исчезает.
- **Жесты (W9):** константы в `gestureConfig.ts` (`EXIT_SLACK_PX=64`, `FLEX_EDGE_DOCK_PX=40`, center band 0.38–0.62, `MAX_SIDE_COLUMNS=2`). Undock — `TitlebarGestureController` / `titlebarGestureMachine`; `cancelFlexLayoutDrag` + `filterLayoutAction` (debug `[flex-dock] reject MOVE_NODE for floating tab`) инкапсулированы в контроллере. Edge-dock / center-band по-прежнему в `patches/flexlayout-react+0.7.15.patch` — значения в config обязаны совпадать.

**Миграция:** старые JSON (только вертикальный стек) грузятся как есть; peer-табы по-прежнему нормализуются в стек. Ширины shell &lt; 240 / &gt; 600 clamp’ятся при `migrateShellPrefs`.

**Решения по продуктовым вопросам:**

1. **Collapse:** shell-collapse сайдбара = узкая полоска 40px (как сейчас). Пока колонка expanded, ширина ≥ soft 240 (hard 200 under pressure) даже если панели «схлопнуты» по высоте до titlebar.
2. **Max columns / side:** `floor(600/240) = 2`.
3. **Preview:** всегда центр (`Model C`); панели left/right of Preview через outer shell, не через center FlexLayout.
4. **Out of space (W8):** не H-scroll. `computeShellWidths` приоритеты: preview ≥ **320** → soft ≥ **240**/колонка → shrink до hard **200** → emergency collapse стороны в **40px** strip (не пишется в prefs; восстанавливается при росте окна). Это сознательное уточнение прежнего «no auto-collapse»: только emergency. `canAcceptHorizontalDrop` отклоняет новую колонку, если solver вернул бы emergency. `minWidth` главного окна = **800** (320+2×240).

### 5.4 Патч FloatingWindow

Stock FlexLayout при occlusion/unload может дернуть `beforeunload` и оставить серое пустое окно в Tauri.

- Исходник патча: `frontend/src/patches/flexlayoutFloatingWindow.cjs`
- Накат: `frontend/scripts/patch-flexlayout.cjs` (`postinstall` / `npm run patch:flexlayout`)
- Суть: не закрывать popout на `beforeunload` из-за occlusion; reuse named window; style clone; close-poll
- **Имя окна:** `window.open` name = `flexPopoutWindowName(tabId)` (без `#`). Сырой FlexLayout id `#uuid` в WKWebView обрезается по `#` → все popout’ы делили пустое имя и второй float переиспользовал первое окно.
- **Vite prebundle:** патч также сбрасывает `node_modules/.vite/deps/flexlayout-react.js*`. Иначе dev-сервер продолжает отдавать stock `window.open(url, id)` и multi-float снова ломается. Vite plugin `ensure-flexlayout-float-patch` гоняет скрипт на `buildStart` / `configureServer`.

**После `npm install` патч должен применяться автоматически.** Без него float в Tauri нестабилен. Если второй detached window «съедает» первый — перезапустите Vite (или `npm run patch:flexlayout`).

---

## 6. Float / popout / redock

```
Жест undock (меню / drag titlebar из колонки)
        │
        ▼
floatPanel → Actions.floatTab(tabId)
        │
        ▼
patched FloatingWindow → window.open(popout.html, …)
        │
        ▼
Tauri on_new_window → WebviewWindow "flex-popout-{n}"
  decorations: false, TitleBarStyle::Overlay
        │
        ▼
Portal в popout-document
layoutPanelFactory оборачивает тело в FlexPopoutChrome
  (WindowTitlebar floating: close / min / zoom + ridges)
```

| Действие | Механизм |
|----------|----------|
| Undock drag из колонки | `useFlexTitlebarUndock` → `floatPanel` |
| Close на popout | `dockPanel` → `closeFlexPopoutShell` (этот `flex-popout-*`) + `unFloatTab` |
| Drag popout на left/right dock | JS `setPosition` (latest-wins) + label-bound affinity → `complete_float_drag` → `flex-panel-dock-request` → `redockFlexPanelToSide` |
| Move to other side (меню) | `movePanelBetweenSides` (delete + `addNode` BOTTOM на другую модель) |

**Flex popout:** без OS `startDragging` / без `global_mouseup` (B4c path A).  
**Preview `panel-*`:** по-прежнему `startDragging` для move-only.

Плейсхолдер «Dock back» / «Show window» в dock-колонке **убран** (`onRenderFloatingTabPlaceholder` → пусто). Оставшийся docked sibling занимает колонку (`rebalanceFloatingTabsets`: floating weight ≈ 0, docked ≈ 100). Это улучшает layout при «один floating, один docked», но убирает явную точку возврата в колонке — см. known issue в §13.

Close native `flex-popout-*` не обрабатывается как legacy `panel-*` в `CloseRequested` — возврат в dock идёт через JS chrome / affinity.

---

## 7. Persistence (W7)

### Рабочий путь (то, что реально дергает UI)

```
Старт: load_shell_layout
  → envelope { version:1, savedAt, left, center, right }
  → normalizeLayoutEnvelope (TS) → Model.fromJson (layoutStore)
  → dockAllFloating; notice toast (migrated / bak / corrupt)

Изменения:
  onModelChange → layoutStore.schedulePersist (debounce 500 ms)
  float / dock / move / redock / swap / reset → persistNow
  close → flush-layout → save_shell_layout → layout_flushed

Формат: layout.json envelope; каждая сторона — сырой Model.toJson() (IJsonModel)
Файл: layout.json (+ layout.json.bak; corrupt → layout.json.corrupt-<ts>)
Миграция: flexlayout_{left,right,center}.json → envelope → *.migrated
```

Команды: `load_shell_layout` / `save_shell_layout` / `layout_flushed`  
(`shell_layout_persistence.rs`). Legacy per-side / `flexlayout_state` IPC still registered, unused by product FE.

---

## 8. Chrome / titlebar (визуал)

Один компонент: `frontend/src/shared/ui/WindowTitlebar.tsx` (+ `.module.css`).

| Режим | Состав |
|-------|--------|
| **docked** | □ · 4 ridge-линии · Title · 4 ridge-линии · □ |
| **floating** | close / min / zoom · ridges · Title |

Тема FL: `flexlayout-theme.css` — рамка tabset/tab согласована с Color Lab (titlebar = верх + divider; тело таба = L/R/bottom), чтобы не было «обрезанной» рамки только на полоске табов.

---

## 9. API `LayoutContext` (публичное)

```ts
useLayoutContext() → {
  left, right,          // { model, setModel, isLoading }
  layoutEpoch,
  movePanelBetweenSides(panelComponent, to),
  floatPanel(side, panelComponent),
  dockPanel(side, panelComponent),
  redockFlexPanelToSide(panelComponent, to),
}

useSideLayout('left' | 'right')

// helpers
findTabByComponent(model, component)
findFirstTabSetId(model)
listFlexComponents(model)
listDockedFlexComponents(model)
normalizeSideToVerticalStack(model): boolean
// horizontalDock.ts
maxDockedHorizontalSpan(model): number
sidebarWidthForHorizontalSpan(span, preferred): number
canAcceptHorizontalDrop(...)
```

Слушатель `flex-panel-dock-request` живёт внутри `LayoutProvider`.

---

## 10. Ключевые файлы

| Путь | Роль |
|------|------|
| `frontend/src/contexts/LayoutContext.tsx` | два Model, float/dock/move/redock |
| `frontend/src/components/FlexLayoutContainer.tsx` | `<Layout>`, titlebar, save |
| `frontend/src/app/AppLayout.tsx` | grid, docked / float-host only |
| `frontend/src/app/providers.tsx` | `LayoutProvider` |
| `frontend/src/app/shell/ShellContext.tsx` | ширина / collapse / split prefs |
| `frontend/src/defaults/DefaultLayouts.ts` | дефолты JSON |
| `frontend/src/factories/layoutPanelFactory.tsx` | factory + `isPanelOnFlexLayout` |
| `frontend/src/features/panels/FlexPopoutChrome.tsx` | chrome popout + float-drag |
| `frontend/src/features/panels/useFlexPopoutJsDrag.ts` | JS `setPosition` + label-bound complete |
| `frontend/src/features/panels/flexPopoutRegistry.ts` | panel/DOM → `flex-popout-*` label |
| `frontend/src/features/panels/closeFlexPopoutShell.ts` | close one shell (label-prefer) |
| `frontend/src/features/panels/gestureConfig.ts` | W9 gesture constants |
| `frontend/src/features/panels/titlebarGestureController.ts` | W9 undock + MOVE_NODE guard |
| `frontend/src/features/panels/SidebarCollapseStrip.tsx` | collapsed 40px |
| `frontend/src/features/panels/FloatingHostStrip.tsx` | float-host return strip |
| `frontend/src/shared/ui/WindowTitlebar.tsx` | общий titlebar |
| `frontend/src/shared/styles/flexlayout-theme.css` | тема FL |
| `frontend/public/popout.html` | host FloatingWindow |
| `frontend/patches/flexlayout-react+0.7.15.patch` | patch-package (float + edge-dock + `__FLEX_PATCH__`) |
| `frontend/scripts/verify-flexlayout-patch.cjs` | W3 verify gate |
| `frontend/src/features/panels/flexlayoutPatchGate.ts` | runtime float allow / overlay |
| `src-tauri/src/commands/flexlayout.rs` | `load_shell_layout` / `save_shell_layout` / `layout_flushed` |
| `src-tauri/src/shell_layout_persistence.rs` | `layout.json` + legacy migrate |
| `src-tauri/src/commands/panels.rs` | affinity → `flex-panel-dock-request` |
| `src-tauri/src/main.rs` | `on_new_window` → `flex-popout-*` |

---

## 11. Как добавить новую панель на FlexLayout

1. Реализовать feature-компонент (контент панели).
2. В `layoutPanelFactory`: case по component id, `hideChrome`, при необходимости wrap `FlexPopoutChrome` для popout.
3. Расширить `isPanelOnFlexLayout`.
4. Добавить в `DefaultLayouts` / миграционную вставку таба, если панель должна быть «из коробки».
5. В Rust: для drag-redock добавить id в `is_flex_layout_panel` (ветка `flex-panel-dock-request`), **не** заводить новый `panel-*` путь.
6. Прогнать: dock, float, close→dock, drag-redock на left/right, move between sides, restart + persist.

---

## 12. Расхождения с ADR B2 — разбор, не таблица «фактов»

ADR — целевое состояние после **полной** миграции. As-built часть отклонений **правильно** адаптировал под shell; часть — **долг**, который нельзя называть архитектурой.

### 12.1 Оправданная эволюция (оставить)

| Отклонение | Почему это нормально |
|------------|----------------------|
| **Два Model (left/right), не один app-wide** | ADR писал «один Model» до того, как зафиксировали, что shell уже владеет двумя колонками как отдельной концепцией. Один Model поверх двухколоночного shell потребовал бы эмулировать стороны внутри единого дерева. Два Model зеркалят реальный UI — честнее слепого следования ADR. |
| **Сырой `Model.toJson()` внутри envelope** | Стороны остаются IJsonModel; versioning — на оболочке `layout.json` (`version: 1`, W7). |
| **Один `layout.json` на три Model** | W7: atomic multi-side write; legacy per-side files migrate once. |

### 12.2 Технический долг и regression risk (не путать с «архитектурой»)

| Отклонение | Честная оценка |
|------------|----------------|
| **Dual SoT layout** (FL + PanelManager для colorlab) | **Закрыто в B4b.** |
| **`global_mouseup` + OS `startDragging` для Flex redock** | **Закрыто в B4c (path A):** JS `setPosition` + in-WebView mouseup; `global_mouseup.rs` удалён. Linux drag-redock больше не зависит от NSEvent/GetAsyncKeyState. Hit-test зон (`dock_affinity` + `update_dock_zone`) оставлен как тонкий мост — не platform mouse sync. |
| **Полная замена PanelManager / урезание panel commands** | **Закрыто.** PanelManager / `panel_state.json` / `panel-*` удалены. Preferences + Help остаются dialogs. Workspace presets → Flex + shell. |
| **flexlayout-react 0.7.15 вместо ~0.10.x из ADR** | **Сознательный пин 0.7.15 (W3, 2026-11-11).** Exact `"0.7.15"` in `frontend/package.json`; Tauri float/edge-dock changes live in `frontend/patches/flexlayout-react+0.7.15.patch` (`patch-package` postinstall) with `__FLEX_PATCH__` gate. Bump to 0.10.x is a **separate spike** (W10) — re-validate the patch; do not silent-bump. |
| **Снимок MIT LICENSE** | Закрыто: [`docs/legal/flexlayout-license-snapshot.md`](./legal/flexlayout-license-snapshot.md) (+ сырой [`flexlayout-LICENSE.MIT.txt`](./legal/flexlayout-LICENSE.MIT.txt)). npm metadata 0.7.15 пишет ISC; текст `LICENSE` — MIT (SoT = файл). |

---

## 13. Known issues / ограничения / следующий шаг

### Float / redock stabilization (2026-11)

Identity + drag lag + ghost chrome were fixed without a layout rewrite. Spec: [`layaut-fix.md`](./layaut-fix.md).

| Seam | Fix |
|------|-----|
| Multi-popout wrong shell on complete/close | Drag binds `flex-popout-*` label into `DockAffinityController`; `poll_panel_affinity` / `handle_panel_moved` / `closeFlexPopoutShell` prefer that label (`flexPopoutRegistry` on the FE). |
| Sticky / stuck JS drag (**W1**) | Pointer capture + explicit state machine (`flexPopoutDragMachine`); terminators include `buttons===0`, watchdog 1.5s, hard cap 60s; `finish(reason)` is idempotent; `complete`/`cancel` take a label. |
| Residual strip / orphan shell / stuck drop-edge | `rebalanceFloatingTabsets` + `[data-flex-float-host]` CSS; close logs when label resolve fails; cancel/complete always emit disarm `dock-affinity`. |
| Undock size clamp | `on_new_window` uses neutral `flex-popout` bounds (not hardcoded `layers`). |

**W1 status:** implemented (machine + pointer capture + label-idempotent IPC). Manual: fast drag, release outside the popout — drop-edge clears, window stops.

**W2 status:** implemented — load-time `dockAllFloating`; OS `Destroyed` → `flex-popout-destroyed` docks tab; float-host `FloatingHostStrip` (focus / double-click return / Return); Window menu + preview “Return to canvas”. Manual: float all → kill app → relaunch; float-host return strip.

**W3 status:** implemented — deliberate pin `flexlayout-react@0.7.15`; `patch-package` + `patches/flexlayout-react+0.7.15.patch`; `npm run verify:flexlayout-patch`; CI step; DEV overlay / PROD float disable.

**W4 status:** implemented — `frontend/src/contexts/layoutStore.ts` owns Models (HMR-preserved singleton); `LayoutProvider` is `useSyncExternalStore` only; `closeAllFlexPopouts` before load/replace/reset; product `Model.fromJson` only via `modelFromJson` in the store (guard test). Manual: HMR with open popout.

**W5 status:** implemented — Rust `flex_popout_lifecycle` (main close/reload/startup scrub); `useOwnerDocument` + listener ownership guard; `flexPopoutStyleSync` head/theme mirror into popouts. DPI/physical pixels → W6. Manual: Cmd/Ctrl+R with three popouts → zero shells; HMR/theme update in an open popout.

**W6 status:** implemented — physical dock zones + `seq` latest-wins; `flexPopoutRegistry` SoT (no any/first close); DEV invariants. Manual: HiDPI / negative-origin monitors.

**W7 status:** implemented — single `layout.json` v1 envelope (`shell_layout_persistence`); atomic write + `.bak` + corrupt quarantine; legacy `flexlayout_{left,right,center}.json` migrate→`*.migrated`; FE normalizer + envelope save via `layoutStore` (`schedulePersist` / `persistNow` / `flushForClose`). Deprecated per-side IPC unused by product UI. Manual: kill-after-move persistence.

**W8 status:** implemented — `computeShellWidths` (soft 240 / hard 200 / emergency 40 strip, not prefs); AppLayout visual emergency collapse; `canAcceptHorizontalDrop` + drop-outline deny when extra column would emergency-collapse; `tauri.conf.json` `minWidth: 800`. Manual: resize 1920→min, no H-scroll, preview ≥320.

**W9 status:** implemented — `gestureConfig.ts`; `titlebarGestureMachine` + `TitlebarGestureController` + `useTitlebarGestureController`; floating `MOVE_NODE` rejected with debug log; table tests (in-column / EXIT_SLACK / post-undock). Patch edge-dock/center-band mirrored in config.

**W10 status:** implemented — AppLayout docked/float-host only (no DockedSidebar / Mixed / Legacy); deprecated per-side layout IPC unregistered; `flexlayout_persistence` module removed (migrate remains in `shell_layout_persistence`); CI adds typecheck + `lint:security`. Unit coverage: W1/W7/W8/W9 + Rust persistence/zones. **Automated tauri-driver e2e** not yet in CI (manual matrix below). **Spike (not this PR):** bump `flexlayout-react` 0.7.15 → 0.10.x and re-patch.

### Persistence (W7)

| Item | Detail |
|------|--------|
| File | `{app_data}/layout.json` |
| Shape | `{ version: 1, savedAt, left, center, right }` — each side is FlexLayout `Model.toJson()` |
| Write | temp → fsync → copy prior to `layout.json.bak` → rename |
| Read | `layout.json` → `layout.json.bak` → migrate legacy side files → defaults |
| Corrupt | rename to `layout.json.corrupt-<ts>`; toast; recover bak or defaults |
| FE | debounce 500 ms on model change; immediate on float/dock/move/redock/swap/reset; flush on close |

### Manual QA matrix (W10 / multi-float) — run on macOS, Windows, Linux

| # | Scenario | Pass criteria |
|---|----------|---------------|
| 1 | Float one panel → drag near left/right edge → redock | No leftover strip; no empty OS window |
| 2 | Float two panels → drag one → redock | Sibling stays up; affinity never arms idle sibling |
| 3 | Float all on one side → redock via close and via affinity | Drop-edge armed style clears on release |
| 4 | Fast drag across monitors (negative origins) | Latest-wins; no backlog feel |
| 5 | Titlebar undock past EXIT_SLACK; mouseup on edge after float | Exactly one float; live popout not MOVE_NODE'd |
| 6 | Resize window 1920 → minWidth (800) with 1+1 / 2+1 columns | Preview ≥320; no shell H-scroll; emergency strip restores on grow |
| 7 | Kill app with floats / move panels → relaunch | All panels docked from `layout.json`; no orphan `flex-popout-*` |
| 8 | Preview float → close / Return to canvas | Canvas restored; no ghost window |
| 9 | Linux smoke | Same float/redock path; no `global_mouseup` |

### Обязательно до релиза (не «потом»)

1. **UX: всё зафлоатил, нет docked sibling.** Колонка = float-host (`FloatingHostStrip`). Возврат: close / drag affinity / меню Return all. **Discoverability** — прогнать в QA для трёх панелей.
2. **Manual QA:** matrix above on all three OSes (automated e2e harness still a follow-up).

### Прочие известные пределы

- Нет нативного drag панели из left Model в right Model — только меню / `movePanelBetweenSides`.
- Popout = тот же JS-дерево через portal в `flex-popout-*`, не отдельный React root.
- Патч FloatingWindow обязателен для Tauri (occlusion / `beforeunload`).
- Опционально позже: один Model на весь shell — только если продуктово понадобится cross-column native drag; сейчас два Model — предпочтительная модель.

### Следующий продуктовый шаг

- **B4b:** ✅ Color Lab на FlexLayout.
- **B4c:** ✅ JS popout drag; `global_mouseup` удалён.
- **B5:** ✅ Preview на center FlexLayout (float = flex-popout; close → canvas).
- **PanelManager retirement:** ✅ leftover SoT удалён; Preferences/Help остаются dialogs; workspace presets → Flex + shell.
- **Float/redock stabilization:** ✅ label-bound affinity + latest-wins drag + ghost cleanup.
- **Spike (separate card):** bump `flexlayout-react` 0.7.15 → 0.10.x, re-run patch, record outcome in this doc regardless of success.
- **Follow-up:** Playwright / tauri-driver e2e smoke on Linux CI (float one / two / redock / restart+persist).

---

## 14. Связанные документы

| Документ | Назначение |
|----------|------------|
| [`legal/flexlayout-license-snapshot.md`](./legal/flexlayout-license-snapshot.md) | Снимок MIT LICENSE для 0.7.15 |
