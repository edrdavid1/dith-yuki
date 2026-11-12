#!/usr/bin/env node
/**
 * W3: fail if flexlayout-react is missing the Tauri float/edge-dock patch marker.
 *
 * Exit codes:
 *   0 — patch present (and Vite prebundle ok or absent)
 *   1 — node_modules patch missing / wrong
 *   2 — node_modules ok but Vite prebundle is stale (caller should clear .vite/deps)
 *
 * Usage: node scripts/verify-flexlayout-patch.cjs [--json]
 */
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const flRoot = path.join(root, 'node_modules/flexlayout-react');
const floatingPath = path.join(flRoot, 'lib/view/FloatingWindow.js');
const indexPath = path.join(flRoot, 'lib/index.js');
const viteBundle = path.join(root, 'node_modules/.vite/deps/flexlayout-react.js');

const EXPECTED_VERSION = 1;
const EXPECTED_FEATURES = [
  'float-window',
  'named-popout',
  'edge-dock',
  'center-band',
];

function fail(msg, code = 1) {
  console.error(`[verify:flexlayout-patch] ${msg}`);
  process.exit(code);
}

function readMarkerFromSource(src) {
  if (!src.includes('__FLEX_PATCH__')) return null;
  if (!src.includes('flexPopoutWindowName')) return null;
  // Prefer evaluating the CJS export when the file is the real module.
  return true;
}

if (!fs.existsSync(floatingPath)) {
  fail(
    `flexlayout-react FloatingWindow missing at ${floatingPath}. Run npm install (with scripts) so patch-package applies patches/flexlayout-react+0.7.15.patch.`,
  );
}

const floatingSrc = fs.readFileSync(floatingPath, 'utf8');
if (!readMarkerFromSource(floatingSrc)) {
  fail(
    'FloatingWindow.js has no __FLEX_PATCH__ / flexPopoutWindowName — stock flexlayout-react detected (e.g. npm ci --ignore-scripts).',
  );
}

let marker;
try {
  // Resolve from installed package (same as runtime).
  const mod = require(floatingPath);
  marker = mod.__FLEX_PATCH__;
} catch (err) {
  fail(`could not require FloatingWindow.js: ${err}`);
}

if (!marker || typeof marker !== 'object') {
  fail('__FLEX_PATCH__ export missing or invalid');
}
if (marker.version !== EXPECTED_VERSION) {
  fail(
    `__FLEX_PATCH__.version=${marker.version} expected ${EXPECTED_VERSION}`,
  );
}
const features = Array.isArray(marker.features) ? marker.features : [];
for (const f of EXPECTED_FEATURES) {
  if (!features.includes(f)) {
    fail(`__FLEX_PATCH__.features missing "${f}" (have: ${features.join(',')})`);
  }
}

if (fs.existsSync(indexPath)) {
  const indexSrc = fs.readFileSync(indexPath, 'utf8');
  if (!indexSrc.includes('__FLEX_PATCH__')) {
    fail('lib/index.js does not re-export __FLEX_PATCH__');
  }
}

let viteStale = false;
if (fs.existsSync(viteBundle)) {
  const viteSrc = fs.readFileSync(viteBundle, 'utf8');
  // Prebundle may minify names — require our distinctive symbols.
  if (
    !viteSrc.includes('flexPopoutWindowName') &&
    !viteSrc.includes('__FLEX_PATCH__')
  ) {
    viteStale = true;
    console.warn(
      '[verify:flexlayout-patch] Vite prebundle looks stock/stale — clearing',
    );
    // Clear here so predev/pretest/CI stay green; Vite plugin also reacts to viteStale.
    require('./clear-vite-flexlayout.cjs');
  }
}

if (process.argv.includes('--json')) {
  console.log(
    JSON.stringify({
      ok: true,
      marker,
      viteStale,
      floatingPath,
    }),
  );
} else {
  console.log(
    `[verify:flexlayout-patch] ok v${marker.version} features=[${features.join(',')}]`,
  );
}

process.exit(0);
