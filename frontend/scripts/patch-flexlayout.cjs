#!/usr/bin/env node
/**
 * Install Tauri-safe FlexLayout FloatingWindow into node_modules.
 * Usage: node scripts/patch-flexlayout.cjs  (also via postinstall)
 */
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const src = path.join(root, 'src/patches/flexlayoutFloatingWindow.cjs');
const dest = path.join(
  root,
  'node_modules/flexlayout-react/lib/view/FloatingWindow.js',
);

if (!fs.existsSync(src)) {
  console.warn('[patch:flexlayout] missing', src);
  process.exit(0);
}
if (!fs.existsSync(path.dirname(dest))) {
  console.warn('[patch:flexlayout] flexlayout-react not installed; skip');
  process.exit(0);
}
fs.copyFileSync(src, dest);
console.log('[patch:flexlayout] applied FloatingWindow.js');
