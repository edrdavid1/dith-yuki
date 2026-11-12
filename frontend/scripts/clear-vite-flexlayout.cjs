#!/usr/bin/env node
/** Drop Vite's prebundled flexlayout-react so the next optimize picks up the patch. */
const fs = require('fs');
const path = require('path');

const viteDeps = path.join(__dirname, '..', 'node_modules/.vite/deps');
if (!fs.existsSync(viteDeps)) {
  process.exit(0);
}

let cleared = false;
for (const name of ['flexlayout-react.js', 'flexlayout-react.js.map']) {
  const p = path.join(viteDeps, name);
  try {
    fs.unlinkSync(p);
    cleared = true;
  } catch {
    /* absent */
  }
}

const metaPath = path.join(viteDeps, '_metadata.json');
if (fs.existsSync(metaPath)) {
  try {
    const meta = JSON.parse(fs.readFileSync(metaPath, 'utf8'));
    if (meta.optimized && meta.optimized['flexlayout-react']) {
      delete meta.optimized['flexlayout-react'];
      fs.writeFileSync(metaPath, JSON.stringify(meta, null, 2));
      cleared = true;
    }
  } catch (err) {
    console.warn('[clear-vite-flexlayout] metadata update failed:', err);
  }
}

if (cleared) {
  console.log('[clear-vite-flexlayout] cleared flexlayout-react from Vite deps');
}
