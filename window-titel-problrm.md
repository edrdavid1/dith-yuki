Ниже — детальная спецификация для ИИ-агента, который будет реализовывать фикс. Структура: общая архитектура → платформенные ветки (macOS / Windows) → конкретные файлы и код → чеклист тестирования.

---

## Контекст для агента

Приложение на Tauri, кросс-платформенное (macOS + Windows), кастомный (frameless) titlebar, в окне есть `<canvas>` с `willChange: 'transform'` для рендеринга контента. На macOS drag через `data-tauri-drag-region` не работает из-за создания отдельного hardware-accelerated CALayer поверх canvas в WKWebView. На Windows механизм рендеринга другой (WebView2/Chromium), поэтому причина бага там иная (или отсутствует), но titlebar всё равно нужно реализовать корректно и с той же архитектурой, чтобы не плодить platform-specific костыли.

**Главный принцип фикса: geometric separation.** Зона тайтлбара физически не должна содержать canvas/GPU-слои под собой ни на одной платформе. Это устраняет саму причину конфликта, а не борется с её платформенными проявлениями по отдельности.

---

## Шаг 1. Общая DOM/layout архитектура (одна для обеих платформ)

```
<div id="app-root">
  <div id="titlebar" data-tauri-drag-region>       <!-- height: 32px, top: 0 -->
    <div id="titlebar-left">...</div>
    <div id="titlebar-controls">                    <!-- см. Шаг 4 -->
      <button data-tauri-drag-region="false" id="btn-minimize" />
      <button data-tauri-drag-region="false" id="btn-maximize" />
      <button data-tauri-drag-region="false" id="btn-close" />
    </div>
  </div>
  <div id="canvas-container" style="position: absolute; top: 32px; left:0; right:0; bottom:0;">
    <canvas ... />
  </div>
</div>
```

**Обязательное условие:** canvas **никогда** не занимает `top: 0` — только `top: var(--titlebar-height)`. Никакого overlay canvas поверх titlebar-зоны, даже прозрачного. Это должно быть закреплено на уровне layout-компонента, а не полагаться на z-index/pointer-events трюки.

Создать переиспользуемый компонент, обязательный для **любого** окна приложения (main, floating preview, будущие окна):

```tsx
// src/components/AppTitlebar.tsx
export function AppTitlebar({ children }: { children?: React.ReactNode }) {
  return (
    <div
      className="app-titlebar"
      data-tauri-drag-region
      style={{
        height: 'var(--titlebar-height)',
        WebkitAppRegion: 'drag', // см. Шаг 2
      } as React.CSSProperties}
    >
      {children}
      <WindowControls /> {/* см. Шаг 4 */}
    </div>
  );
}
```

Все окна должны оборачиваться в общий layout:

```tsx
// src/components/WindowShell.tsx
export function WindowShell({ children }: { children: React.ReactNode }) {
  return (
    <>
      <AppTitlebar />
      <div className="content-area" style={{ marginTop: 'var(--titlebar-height)' }}>
        {children}
      </div>
    </>
  );
}
```

Это архитектурно исключает ситуацию «забыли добавить titlebar в новое окно» — новое окно просто не будет валидным без `WindowShell`.

---

## Шаг 2. macOS: нативный `-webkit-app-region` вместо `data-tauri-drag-region`

`data-tauri-drag-region` — это JS-эмуляция (Tauri вешает `mousedown`-listener и вызывает `startDragging()`). На WKWebView она уязвима к canvas-слоям. `-webkit-app-region: drag` — нативный CSS-хинт, обрабатываемый WebKit/AppKit на уровне hit-testing, минуя JS pipeline.

```css
/* styles/titlebar.css */
.app-titlebar {
  -webkit-app-region: drag;
}
.app-titlebar button,
.app-titlebar input,
.app-titlebar [data-tauri-drag-region="false"] {
  -webkit-app-region: no-drag;
}
```

Важно: `-webkit-app-region` — это **WebKit-specific** свойство, на Windows (WebView2/Chromium) оно не гарантированно поддерживается тем же образом → нужна платформенная ветка (Шаг 3). Поэтому реализуем через platform detection на уровне сборки/рантайма, не полагаясь на graceful degradation CSS.

```ts
// src/lib/platform.ts
import { platform } from '@tauri-apps/plugin-os';

export const CURRENT_PLATFORM = await platform(); // 'macos' | 'windows' | ...
```

```tsx
// AppTitlebar.tsx — платформенная ветка стиля
const dragStyle = CURRENT_PLATFORM === 'macos'
  ? { WebkitAppRegion: 'drag' }
  : {}; // на Windows дальше используем data-tauri-drag-region + доп. фикс (Шаг 3)
```

Оставить `data-tauri-drag-region` атрибут на элементе в обоих случаях — на macOS он становится избыточным (т.к. работает `-webkit-app-region`), но не мешает; на Windows он остаётся основным механизмом.

**Дополнительно на macOS:** убедиться, что `tauri.conf.json` использует `"titleBarStyle": "Overlay"` или `"transparent": true` с `decorations: false` — не смешивать нативный macOS titlebar с кастомным (иначе получите двойной hit-testing слой).

---

## Шаг 3. Windows: WebView2-специфика

На Windows (WebView2/Chromium) причина бага, описанного для macOS, как правило не воспроизводится один в один (Chromium иначе организует compositing и input routing), но есть свои грабли, которые агенту нужно закрыть, чтобы titlebar был production-ready:

### 3.1. `data-tauri-drag-region` остаётся основным механизмом
На Windows Tauri реализует drag через `WM_NCLBUTTONDOWN`/hit-test трюк — это работает надёжно с обычными DOM-элементами. Убедиться, что:
- canvas **не перекрывает** DOM-элемент с `data-tauri-drag-region` геометрически (см. Шаг 1 — уже решено layout-разделением).
- Если canvas всё же должен визуально заходить под titlebar (например, полупрозрачный фон на всё окно) — верхний DOM-слой titlebar должен быть выше canvas в z-index и НЕ иметь `pointer-events: none`, иначе Windows-hit-test не сработает.

### 3.2. Snap Layouts (Windows 11) и maximize-кнопка
На Windows 11 системная фича Snap Layout работает только если кнопка maximize реализована через **системную** kнопку (либо через `decorations: true` + `titleBarStyle`, либо явно эмулируется через Tauri API с hover-триггером). Если используете полностью кастомные HTML-кнопки без интеграции с этим API — Snap Layout не появится при наведении на кнопку maximize. Это не баг, а решение продукта: если нужен Snap Layout, использовать `@tauri-apps/api/window` -> `getCurrentWindow().toggleMaximize()` и задать `data-tauri-drag-region` зоне рядом, но полноценный snap-flyout потребует Windows-native кнопку (см. Tauri issue tracker/`window-vibrancy`/`tauri-plugin-decorum` — переиспользуемый плагин, который умеет рисовать нативные Windows-controls поверх WebView2).

**Рекомендация агенту:** не пытаться реализовать Snap Layout вручную — использовать `tauri-plugin-decorum` (сообществом поддерживаемый плагин именно для кросс-платформенных кастомных titlebar с корректными Windows-controls) либо явно зафиксировать в задаче, что Snap Layout не поддерживается в MVP.

### 3.3. Двойной клик на titlebar → maximize/restore
Нужно реализовать вручную (Tauri v2 сам не делает toggle на дважды клике для кастомных regions):

```ts
// AppTitlebar.tsx
const handleDoubleClick = async () => {
  const win = getCurrentWindow();
  const maximized = await win.isMaximized();
  maximized ? await win.unmaximize() : await win.maximize();
};
```
Навесить на titlebar-элемент одинаково для обеих платформ (на macOS системное поведение double-click уже даёт maximize через `data-tauri-drag-region`, но явный fallback не мешает — проверить, не задвоится ли действие, тестом из чеклиста).

### 3.4. DPI и HiDPI-скейлинг
Windows чаще, чем macOS, работает с нецелым scale factor (125%, 150%). Убедиться, что `titlebar-height` задан в logical px через CSS-переменную, а не в canvas device pixels, иначе на не-100% масштабе зона drag и зона canvas разъедутся на 1-2px и появится "дребезг" по границе.

---

## Шаг 4. Оконные кнопки (minimize/maximize/close) — обязательны на Windows, опциональны на macOS

На macOS системные traffic-lights рисуются нативно при `decorations: false` + `titleBarStyle: Overlay` — кастомные HTML-кнопки не нужны (либо не показывать `WindowControls` вообще на macOS).
На Windows при `decorations: false` система ничего не рисует — кнопки **обязательно** реализовать вручную.

```tsx
// src/components/WindowControls.tsx
import { getCurrentWindow } from '@tauri-apps/api/window';

export function WindowControls() {
  if (CURRENT_PLATFORM === 'macos') return null; // нативные traffic-lights уже есть

  const win = getCurrentWindow();
  return (
    <div className="window-controls" data-tauri-drag-region="false">
      <button onClick={() => win.minimize()}>—</button>
      <button onClick={async () => (await win.isMaximized()) ? win.unmaximize() : win.maximize()}>□</button>
      <button onClick={() => win.close()}>×</button>
    </div>
  );
}
```

`tauri.conf.json`:
```jsonc
{
  "app": {
    "windows": [{
      "decorations": false,
      "titleBarStyle": "Overlay" // применяется только на macOS, на Windows игнорируется/decorations:false достаточно
    }]
  }
}
```

---

## Шаг 5. Единая точка правды — не дублировать логику per-window

Как и для viewport-стейта, здесь тоже важно не дать разработчикам/агенту в будущем создавать новые окна без titlebar-обвязки. Зафиксировать в кодовой базе:

- Любое новое Tauri-окно **обязано** рендерить root-компонент через `<WindowShell>`.
- Linter/code-review правило (можно оформить как ESLint custom rule или просто комментарий-конвенцию в `WindowShell.tsx`): «не создавать canvas с `top: 0` в окне, минуя `WindowShell`».
- Все future floating-окна (preview и др.) переиспользуют `AppTitlebar`/`WindowControls` — не форкать отдельную copy-paste реализацию под каждое окно.

---

## Чеклист тестирования для агента

**macOS:**
- [ ] Drag за titlebar работает при активном panning/zoom canvas (willChange активен)
- [ ] Drag работает, когда окно не в фокусе (первый клик должен сразу двигать окно либо фокусировать — задокументировать ожидаемое поведение)
- [ ] Double-click на titlebar → maximize/restore, без задваивания эффекта от ручного обработчика
- [ ] Traffic-lights (native) не перекрываются кастомными элементами интерфейса

**Windows:**
- [ ] Drag за titlebar работает аналогично, canvas не перехватывает клик
- [ ] Custom minimize/maximize/close кнопки работают, `pointer-events` корректны (не dragging при клике на кнопку)
- [ ] Double-click toggle работает
- [ ] Поведение на 125%/150% DPI — нет визуального рассинхрона titlebar/canvas
- [ ] (Если используется `tauri-plugin-decorum` или аналог) Snap Layout появляется при hover на maximize

**Оба:**
- [ ] Новое тестовое floating-окно, созданное через `WindowShell`, сразу получает рабочий drag без доп. кода
- [ ] Resize окна не ломает позиционирование titlebar/canvas-границы