# FlexLayout: интеграция докинга

**Дата:** 2026-09-04  
**Статус:** as-built (B3 + B4a + B4b + B4c)  
**Библиотека:** [`flexlayout-react`](https://github.com/caplin/FlexLayout) **0.7.15** (MIT; см. [`legal/flexlayout-license-snapshot.md`](./legal/flexlayout-license-snapshot.md))  
**ADR:** [`B2_ADR_flexlayout_docking.md`](./B2_ADR_flexlayout_docking.md)  
**Канбан:** [`KANBAN_gpu_and_docking.md`](./KANBAN_gpu_and_docking.md) (трек B)

Этот документ — актуальная картина: как библиотека подключена, какие окна мигрированы, кто чем владеет, как устроены float/redock и persistence.

Расхождения с ADR (§12) разделены на **оправданную эволюцию** и **технический долг** — не смешивать.

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
| `preview` | PreviewSlot / floating-only | Canvas preview | свой undock, не FL |
| `preferences` | диалог / floating-only | — | не FL |

Признак миграции на фронте:

```ts
isPanelOnFlexLayout(id) === (id === 'layers' || id === 'effect' || id === 'colorlab')
```

Фабрика: `frontend/src/factories/layoutPanelFactory.tsx`.  
В `DockedSidebar` flex-панели **фильтруются** — даже если Rust ещё знает про них в старых ордерах `panel_state.json`.

Дефолт при пустом/битом layout:

- **left** → Layers  
- **right** → Effect + Color Lab (вертикальный стек)

(`frontend/src/defaults/DefaultLayouts.ts`)

**B4b persistence:** если у пользователя уже есть `flexlayout_right.json` без `colorlab`, при загрузке таб Color Lab **добавляется** справа (не затирая Layers/Effect); toast: `LAYOUT_TOAST_MESSAGES.COLORLAB_ADDED`.

---

## 3. Кто чем владеет

| Забота | Владелец |
|--------|----------|
| Дерево табов, same-side drag/split, float-геометрия | FlexLayout `Model` (**два** инстанса: left + right) |
| Ширина колонки, collapse, split ratio | `ShellContext` (`dither.shellPrefs` в localStorage) |
| Preview / Preferences | свои пути (не FL) |
| Persist JSON FlexLayout | Tauri `save_layout_{left,right}` / `load_layout_{left,right}` |
| Drag-to-redock (hit-test зон) | Rust `dock_affinity` + zone IPC; **complete** = JS mouseup in popout → `complete_float_drag` (B4c). `global_mouseup` удалён. |

Два Model — осознанно: shell уже делит экран на левую/правую колонку + canvas. Перенос панели между сторонами — не нативный drag FlexLayout, а `movePanelBetweenSides`.

**После B4c:** SoT layout — Flex Model; redock popout — JS drag (path A). Preview по-прежнему OS `startDragging` (FLOATING_ONLY).

```
┌──────────────┬────────────────────┬──────────────┐
│ left column  │  canvas / Preview  │ right column │
│              │                    │              │
│ Model L      │                    │ Model R      │
│ FlexLayout   │                    │ FlexLayout   │
│ (Layers …)   │                    │ (Effect +    │
│              │                    │  Color Lab)  │
└──────────────┴────────────────────┴──────────────┘
         ▲                                      ▲
         └── flex-popout-* JS setPosition + mouseup ──┘
              (affinity hit-test; no global_mouseup)
```

Providers: `ShellProvider` → `ShortcutsProvider` → `LayoutProvider` → `AppLayout`.

---

## 4. Как устроена колонка (`AppLayout`)

На каждую сторону режимы:

| Режим | Условие | UI |
|-------|---------|-----|
| **Flex-only** | есть docked flex-табы, legacy нет | `FlexLayoutContainer` + collapse / resize |
| **Float-host** | flex-табы есть, все floating, legacy нет | почти нулевая ширина / off-screen host + drop-edge для redock |
| **Mixed** | docked flex + legacy | FlexLayout сверху, `DockedSidebar` снизу, shell split ratio |
| **Legacy-only** | docked flex нет, legacy есть | только `DockedSidebar` |
| **Floated flex + legacy** | flex только floating, legacy есть | off-screen FL host + видимый DockedSidebar |

`layoutEpoch` в `LayoutContext` заставляет shell пересчитать ширину/host после float/dock **без** клонирования `Model` (клон ломает popout).

Collapse: `SidebarCollapseStrip` (40px); drag ширины &lt; **220px** → collapse.

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

### 5.3 Вертикальный стек (как Color Lab)

Drop left/center/right внутри колонки в `onAction` переписывается в **`bottom`**.  
`normalizeSideToVerticalStack` разносит peer-табы в один tabset в вертикальный стек. Горизонтальный split внутри сайдбара продуктово не нужен.

### 5.4 Патч FloatingWindow

Stock FlexLayout при occlusion/unload может дернуть `beforeunload` и оставить серое пустое окно в Tauri.

- Исходник патча: `frontend/src/patches/flexlayoutFloatingWindow.cjs`
- Накат: `frontend/scripts/patch-flexlayout.cjs` (`postinstall` / `npm run patch:flexlayout`)
- Суть: не закрывать popout на `beforeunload` из-за occlusion; reuse named window; style clone; close-poll

**После `npm install` патч должен применяться автоматически.** Без него float в Tauri нестабилен.

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
| Close на popout | `dockPanel` → `unFloatTab` |
| Drag popout на left/right dock | JS `setPosition` + affinity → `complete_float_drag` → `flex-panel-dock-request` → `redockFlexPanelToSide` |
| Move to other side (меню) | `movePanelBetweenSides` (delete + `addNode` BOTTOM на другую модель) |

**Flex popout:** без OS `startDragging` / без `global_mouseup` (B4c path A).  
**Preview `panel-*`:** по-прежнему `startDragging` для move-only.

Плейсхолдер «Dock back» / «Show window» в dock-колонке **убран** (`onRenderFloatingTabPlaceholder` → пусто). Оставшийся docked sibling занимает колонку (`rebalanceFloatingTabsets`: floating weight ≈ 0, docked ≈ 100). Это улучшает layout при «один floating, один docked», но убирает явную точку возврата в колонке — см. known issue в §13.

Close native `flex-popout-*` не обрабатывается как legacy `panel-*` в `CloseRequested` — возврат в dock идёт через JS chrome / affinity.

---

## 7. Persistence

### Рабочий путь (то, что реально дергает UI)

```
Старт: load_layout_left / load_layout_right
  → Model.fromJson → applyAppChromePolicy
  → ошибка парса → DefaultLayouts (TS)

Изменения:
  onModelChange → debounce 500 ms → save_layout_{side}
  float / dock / move → immediate persistSide

Формат на диске: сырой Model.toJson() (IJsonModel), без обёртки { version: 3, … }
Файлы: flexlayout_left.json, flexlayout_right.json (app data dir)
```

Команды: `src-tauri/src/commands/flexlayout.rs` + `flexlayout_persistence.rs`.

### Нюансы vs ADR

ADR предлагал обёртку `{ version: 3, flexlayout_model }` и один файл. As-built — **два файла по сторонам**, сырой JSON (оправданное упрощение, §12.1). Глобальные `save_layout` / `load_layout` / `flexlayout_state.json` в Rust ещё есть, но side-load фронта ими не пользуется.

Миграции v2 `panel_state.json` → точный FlexLayout tree **нет**: при проблемах — дефолт из `DefaultLayouts`.

**На будущее:** без поля `version` в файле bump формата придётся вводить постфактум («файлы без version = предполагаемая v-нет-номера»). Сейчас не критично; помнить при первой несовместимой смене схемы.

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
```

Слушатель `flex-panel-dock-request` живёт внутри `LayoutProvider`.

---

## 10. Ключевые файлы

| Путь | Роль |
|------|------|
| `frontend/src/contexts/LayoutContext.tsx` | два Model, float/dock/move/redock |
| `frontend/src/components/FlexLayoutContainer.tsx` | `<Layout>`, titlebar, save |
| `frontend/src/app/AppLayout.tsx` | grid, flex/legacy/mixed/float-host |
| `frontend/src/app/providers.tsx` | `LayoutProvider` |
| `frontend/src/app/shell/ShellContext.tsx` | ширина / collapse / split |
| `frontend/src/defaults/DefaultLayouts.ts` | дефолты JSON |
| `frontend/src/factories/layoutPanelFactory.tsx` | factory + `isPanelOnFlexLayout` |
| `frontend/src/features/panels/FlexPopoutChrome.tsx` | chrome popout + float-drag |
| `frontend/src/features/panels/useFlexTitlebarUndock.ts` | undock жестом |
| `frontend/src/features/panels/SidebarCollapseStrip.tsx` | collapsed 40px |
| `frontend/src/features/panels/DockedSidebar.tsx` | legacy shell path (empty for dockables after B4b) |
| `frontend/src/shared/ui/WindowTitlebar.tsx` | общий titlebar |
| `frontend/src/shared/styles/flexlayout-theme.css` | тема FL |
| `frontend/public/popout.html` | host FloatingWindow |
| `frontend/src/patches/flexlayoutFloatingWindow.cjs` | патч popout |
| `frontend/scripts/patch-flexlayout.cjs` | postinstall |
| `src-tauri/src/commands/flexlayout.rs` | IPC save/load |
| `src-tauri/src/flexlayout_persistence.rs` | диск |
| `src-tauri/src/commands/panels.rs` | affinity → `flex-panel-dock-request` |
| `src-tauri/src/main.rs` | `on_new_window` → `flex-popout-*` |

---

## 11. Как добавить новую панель на FlexLayout

1. Реализовать feature-компонент (контент панели).
2. В `layoutPanelFactory`: case по component id, `hideChrome`, при необходимости wrap `FlexPopoutChrome` для popout.
3. Расширить `isPanelOnFlexLayout` (и при необходимости `isPanelDockable`).
4. Добавить в `DefaultLayouts` / миграционную вставку таба, если панель должна быть «из коробки».
5. Убрать id из рендера `DockedSidebar` / фильтра legacy.
6. В Rust: для drag-redock добавить id в `is_flex_layout_panel` (ветка `flex-panel-dock-request`), **не** заводить новый `panel-*` путь.
7. Прогнать: dock, float, close→dock, drag-redock на left/right, move between sides, restart + persist.

---

## 12. Расхождения с ADR B2 — разбор, не таблица «фактов»

ADR — целевое состояние после **полной** миграции. As-built часть отклонений **правильно** адаптировал под shell; часть — **долг**, который нельзя называть архитектурой.

### 12.1 Оправданная эволюция (оставить)

| Отклонение | Почему это нормально |
|------------|----------------------|
| **Два Model (left/right), не один app-wide** | ADR писал «один Model» до того, как зафиксировали, что shell уже владеет двумя колонками как отдельной концепцией. Один Model поверх двухколоночного shell потребовал бы эмулировать стороны внутри единого дерева. Два Model зеркалят реальный UI — честнее слепого следования ADR. |
| **Сырой `Model.toJson()` вместо `{ version: 3, flexlayout_model }`** | Упрощение ради доверия к сериализации библиотеки, не жёсткое требование. Риск: без обёртки versioning формата придётся вводить постфактум (см. §7). |
| **Per-side файлы** (`flexlayout_left/right.json`) | Следствие двух Model; согласовано с shell. |

### 12.2 Технический долг и regression risk (не путать с «архитектурой»)

| Отклонение | Честная оценка |
|------------|----------------|
| **Dual SoT layout** (FL + PanelManager для colorlab) | **Закрыто в B4b.** |
| **`global_mouseup` + OS `startDragging` для Flex redock** | **Закрыто в B4c (path A):** JS `setPosition` + in-WebView mouseup; `global_mouseup.rs` удалён. Linux drag-redock больше не зависит от NSEvent/GetAsyncKeyState. Hit-test зон (`dock_affinity` + `update_dock_zone`) оставлен как тонкий мост — не platform mouse sync. |
| **Полная замена PanelManager / урезание panel commands** | Preview/Preferences + presets ещё на PanelManager. Не блокер redock. |
| **flexlayout-react 0.7.15 вместо ~0.10.x из ADR** | На B3/spike в lockfile попал точный пин **0.7.15** (последний 0.7.x; peer React 18). Spike-лог фиксирует «0.7.15 integrates cleanly», **без** сравнения с 0.10.x и без записи «0.10 сломался → откат». На дату установки (≈2026-08-31) линейка 0.10.x уже была на npm. **Вердикт: незакрытый апдейт / неуточнённый пин, не документированный сознательный откат.** Перед bump — заново прогнать popout + патч FloatingWindow на целевой версии. |
| **Снимок MIT LICENSE** | Закрыто: [`docs/legal/flexlayout-license-snapshot.md`](./legal/flexlayout-license-snapshot.md) (+ сырой [`flexlayout-LICENSE.MIT.txt`](./legal/flexlayout-LICENSE.MIT.txt)). npm metadata 0.7.15 пишет ISC; текст `LICENSE` — MIT (SoT = файл). |

---

## 13. Known issues / ограничения / следующий шаг

### Обязательно до релиза (не «потом»)

1. **UX: всё зафлоатил, нет docked sibling.** Плейсхолдер «Dock back» убран; колонка уходит в float-host. Возврат: close на `FlexPopoutChrome`, drag на dock-зону (JS affinity), либо меню. **Discoverability** — прогнать в QA для трёх панелей.
2. **B4c manual QA:** feel JS `setPosition` drag vs старый OS-drag; negative-origin multi-monitor; Linux smoke redock (спека [`.cursor-spec/track-r-docking/B4c_js_popout_drag_spec.md`](../.cursor-spec/track-r-docking/B4c_js_popout_drag_spec.md)).

### Прочие известные пределы

- Нет нативного drag панели из left Model в right Model — только меню / `movePanelBetweenSides`.
- Popout = тот же JS-дерево через portal в `flex-popout-*`, не отдельный React root.
- Патч FloatingWindow обязателен для Tauri (occlusion / `beforeunload`).
- Опционально позже: один Model на весь shell — только если продуктово понадобится cross-column native drag; сейчас два Model — предпочтительная модель.

### Следующий продуктовый шаг

- **B4b:** ✅ Color Lab на FlexLayout.
- **B4c:** ✅ JS popout drag; `global_mouseup` удалён.
- **Опционально:** дальше сузить PanelManager (Preview-only) / workspace presets → Flex; bump `flexlayout-react` 0.7.15 → 0.10.x или явно зафиксировать пин.

---

## 14. Связанные документы

| Документ | Назначение |
|----------|------------|
| [`B2_ADR_flexlayout_docking.md`](./B2_ADR_flexlayout_docking.md) | Решение «почему FlexLayout» и целевая архитектура |
| [`FLEXLAYOUT_DOCKING_AS_BUILT.md`](./FLEXLAYOUT_DOCKING_AS_BUILT.md) | Короткий English index → этот файл |
| [`legal/flexlayout-license-snapshot.md`](./legal/flexlayout-license-snapshot.md) | Снимок MIT LICENSE для 0.7.15 |
| `.cursor-spec/track-b-infra/AUDIT_docking_current_implementation.md` | B1: affinity / Linux |
| `.cursor-spec/track-r-docking/SPIKE_EXECUTION_LOG.md` | Spike: Path A + пин 0.7.15 |
| `frontend/src/FLEXLAYOUT_INTEGRATION_GUIDE.md` | **Устарел** (B3 Layers-only) |
