#!/usr/bin/env node
/**
 * Extract App.css line ranges into target files, then rewrite App.css without those ranges.
 * Usage: node scripts/extract-css-sections.mjs
 */
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve('src');
const appCssPath = path.join(root, 'App.css');
const lines = fs.readFileSync(appCssPath, 'utf8').split('\n');

/** @type {{ out: string, start: number, end: number, transform?: (css: string) => string }[]} */
const extractions = [
  // Layers Panel Figma
  {
    out: 'features/layers/LayersPanel.module.css',
    start: 1302,
    end: 1698,
    transform: (css) =>
      css
        // parent still global during early migrate; then panels module
        .replaceAll('.panel-window-content .lp', ':global(.panel-window-content) .lp')
        .replace(
          '.lp-layers-area .simplebar-scrollable-y .simplebar-content-wrapper',
          '.lp-layers-area :global(.simplebar-scrollable-y .simplebar-content-wrapper)'
        ),
  },
  // Effect chooser dialog
  { out: 'features/effects/EffectChooserDialog.module.css', start: 1699, end: 1771 },
  // Effect chooser inline + settings
  {
    out: 'features/effects/EffectSettingsPanel.module.css',
    start: 1772,
    end: 1877,
    transform: (css) =>
      css
        .replaceAll(
          '.panel-window-content .effect-settings-panel',
          ':global(.panel-window-content) .effect-settings-panel'
        )
        .replaceAll(
          '.panel-window-content .effect-chooser-panel',
          ':global(.panel-window-content) .effect-chooser-panel'
        )
        .replace(
          '.effect-settings-scroll .simplebar-scrollable-y .simplebar-content-wrapper',
          '.effect-settings-scroll :global(.simplebar-scrollable-y .simplebar-content-wrapper)'
        ),
  },
  // Param input (shared by effects editors + palette)
  { out: 'shared/ui/ParamInput.module.css', start: 1878, end: 1902 },
  // Slider control
  { out: 'shared/ui/Slider.module.css', start: 787, end: 854 },
  // Retro slider bits used by Slider + Layers opacity
  { out: 'shared/ui/RetroSlider.module.css', start: 615, end: 696 },
  // Param controls (param-group, swatch boxes, etc.)
  { out: 'shared/ui/ParamControls.module.css', start: 855, end: 929 },
  // Curves editor
  { out: 'features/effects/editors/CurvesSettings.module.css', start: 930, end: 956 },
  // Filter buttons
  { out: 'shared/ui/FilterButtons.module.css', start: 712, end: 739 },
  // Resize handles
  { out: 'shared/ui/ResizeHandle.module.css', start: 47, end: 111 },
  // Window titlebar shared
  {
    out: 'shared/ui/WindowTitlebar.module.css',
    start: 421,
    end: 555,
  },
  // Preview canvas + window
  { out: 'features/preview/Preview.module.css', start: 272, end: 323 },
  { out: 'features/preview/PreviewWindow.module.css', start: 349, end: 420 },
  // Empty + Notification
  { out: 'shared/ui/EmptyState.module.css', start: 1028, end: 1048 },
  { out: 'shared/ui/Notification.module.css', start: 1049, end: 1095 },
  // Menu
  { out: 'features/document/MenuBar.module.css', start: 133, end: 271 },
  // App layout + sidebar + collapsed + drop
  {
    out: 'app/AppLayout.module.css',
    start: 112,
    end: 132,
  },
];

const used = new Set();
for (const ex of extractions) {
  for (let i = ex.start; i <= ex.end; i++) used.add(i);
}

function slice(start, end) {
  // 1-indexed inclusive
  return lines.slice(start - 1, end).join('\n').trimEnd() + '\n';
}

for (const ex of extractions) {
  let css = slice(ex.start, ex.end);
  if (ex.transform) css = ex.transform(css);
  const outPath = path.join(root, ex.out);
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  // Append if file already exists from earlier pass
  if (fs.existsSync(outPath) && fs.readFileSync(outPath, 'utf8').includes(css.slice(0, 40))) {
    console.log('skip duplicate', ex.out);
    continue;
  }
  fs.writeFileSync(outPath, css);
  console.log('wrote', ex.out, `(${ex.end - ex.start + 1} lines)`);
}

// Additional extractions appended into AppLayout
const moreAppLayout = [
  [324, 348], // sidebar + split
  [2320, 2424], // drop indicator + collapsed
];
let appLayout = fs.readFileSync(path.join(root, 'app/AppLayout.module.css'), 'utf8');
for (const [s, e] of moreAppLayout) {
  appLayout += '\n' + slice(s, e);
  for (let i = s; i <= e; i++) used.add(i);
}
fs.writeFileSync(path.join(root, 'app/AppLayout.module.css'), appLayout);
console.log('appended sidebar/collapsed to AppLayout.module.css');

// Palette + color picker + color lab buttons
const colorLabBits = [
  [1903, 1966, 'features/color-lab/PalettePanel.module.css'],
  [1967, 1996, 'features/color-lab/PaletteSwatches.module.css'],
  [2012, 2043, 'features/color-lab/ColorLabButtons.module.css'],
  [2044, 2171, 'features/color-lab/ColorPicker.module.css'],
];
for (const [s, e, out] of colorLabBits) {
  for (let i = s; i <= e; i++) used.add(i);
  const outPath = path.join(root, out);
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, slice(s, e));
  console.log('wrote', out);
}

// Panel window section from App.css (also have PanelWindow.css)
for (let i = 2172; i <= 2319; i++) used.add(i);
fs.writeFileSync(
  path.join(root, 'features/panels/PanelChrome.module.css'),
  slice(2172, 2319)
);
console.log('wrote features/panels/PanelChrome.module.css');

// Legacy layers (keep as module next to legacy components for now)
for (const [s, e, out] of [
  [1096, 1188, 'components/LayerPanel.module.css'],
  [1189, 1301, 'components/LayerControls.module.css'],
  [740, 786, 'components/FilterList.module.css'],
  [957, 1027, 'features/preview/ZoomControls.module.css'],
  [1997, 2011, 'features/effects/editors/CurvesPlaceholder.module.css'],
  [556, 614, 'shared/ui/RetroForm.module.css'],
]) {
  for (let i = s; i <= e; i++) used.add(i);
  const outPath = path.join(root, out);
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, slice(s, e));
  console.log('wrote', out);
}

// Mark simplebar section used (already extracted to vendor)
for (let i = 6; i <= 46; i++) used.add(i);

// Rewrite App.css with only unused lines + note
const remaining = [];
remaining.push('/* App.css strangler remnant — domain styles moved to *.module.css */');
remaining.push('');
for (let i = 1; i <= lines.length; i++) {
  if (!used.has(i)) {
    const line = lines[i - 1];
    // skip empty banner-only leftovers if entirely comment sections already moved
    remaining.push(line);
  }
}
const remnant = remaining.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n';
fs.writeFileSync(appCssPath, remnant);
console.log('App.css remaining lines:', remnant.split('\n').length);
