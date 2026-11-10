#!/usr/bin/env node
/**
 * Install Tauri-safe FlexLayout patches into node_modules.
 * Usage: node scripts/patch-flexlayout.cjs  (also via postinstall / Vite buildStart)
 *
 * 1) FloatingWindow — named window.open + occlusion-safe close
 * 2) Wider column edge-dock hit strip (stock is 10px — nearly unusable)
 * 3) Larger within-tabset left/right bands (smaller center dead-zone)
 *
 * Also drops Vite's prebundled flexlayout-react so the running app picks these up.
 */
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const flRoot = path.join(root, 'node_modules/flexlayout-react');
const src = path.join(root, 'src/patches/flexlayoutFloatingWindow.cjs');
const dest = path.join(flRoot, 'lib/view/FloatingWindow.js');

if (!fs.existsSync(src)) {
  console.warn('[patch:flexlayout] missing', src);
  process.exit(0);
}
if (!fs.existsSync(path.dirname(dest))) {
  console.warn('[patch:flexlayout] flexlayout-react not installed; skip');
  process.exit(0);
}
fs.copyFileSync(src, dest);

/** Apply one exact string replace; no-op (with warn) if the stock needle is gone. */
function replaceIn(rel, from, to, label) {
  const file = path.join(flRoot, rel);
  if (!fs.existsSync(file)) {
    console.warn(`[patch:flexlayout] missing ${rel}`);
    return false;
  }
  const before = fs.readFileSync(file, 'utf8');
  if (before.includes(to) && !before.includes(from)) {
    return true; // already patched
  }
  if (!before.includes(from)) {
    console.warn(`[patch:flexlayout] ${label}: needle not found in ${rel}`);
    return false;
  }
  fs.writeFileSync(file, before.replace(from, to));
  return true;
}

// Stock edge dock is a 10×100px mid-edge chip — widen so side-place is hittable.
replaceIn(
  'lib/view/Layout.js',
  'this.edgeRectLength = 100;',
  'this.edgeRectLength = 180;',
  'edgeRectLength',
);
replaceIn(
  'lib/view/Layout.js',
  'this.edgeRectWidth = 10;',
  'this.edgeRectWidth = 40;',
  'edgeRectWidth',
);
replaceIn(
  'lib/model/RowNode.js',
  'const margin = 10; // height of edge rect',
  'const margin = 40; // widened side-dock strip (was 10)',
  'RowNode margin',
);
replaceIn(
  'lib/model/RowNode.js',
  'const half = 50; // half width of edge rect',
  'const half = 90; // half length of side-dock band (was 50)',
  'RowNode half',
);
// Shrink the center 50% square so left/right bands are easier to arm.
replaceIn(
  'lib/DockLocation.js',
  'if (x >= 0.25 && x < 0.75 && y >= 0.25 && y < 0.75) {',
  'if (x >= 0.38 && x < 0.62 && y >= 0.38 && y < 0.62) {',
  'DockLocation center',
);

const viteDeps = path.join(root, 'node_modules/.vite/deps');
let clearedVite = false;
if (fs.existsSync(viteDeps)) {
  for (const name of [
    'flexlayout-react.js',
    'flexlayout-react.js.map',
  ]) {
    const p = path.join(viteDeps, name);
    try {
      fs.unlinkSync(p);
      clearedVite = true;
    } catch {
      /* not present */
    }
  }
  // Drop dep metadata so Vite re-optimizes flexlayout with the patched files.
  const metaPath = path.join(viteDeps, '_metadata.json');
  if (fs.existsSync(metaPath)) {
    try {
      const meta = JSON.parse(fs.readFileSync(metaPath, 'utf8'));
      if (meta.optimized && meta.optimized['flexlayout-react']) {
        delete meta.optimized['flexlayout-react'];
        fs.writeFileSync(metaPath, JSON.stringify(meta, null, 2));
        clearedVite = true;
      }
    } catch {
      /* ignore corrupt metadata */
    }
  }
}

console.log(
  clearedVite
    ? '[patch:flexlayout] applied float + edge-dock patches (cleared Vite prebundle)'
    : '[patch:flexlayout] applied float + edge-dock patches',
);
