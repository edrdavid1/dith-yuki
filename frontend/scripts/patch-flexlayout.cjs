#!/usr/bin/env node
/**
 * DEPRECATED (W3): FlexLayout patches are applied by patch-package from
 * patches/flexlayout-react+0.7.15.patch on postinstall.
 *
 * Source reference for the FloatingWindow body still lives at
 * src/patches/flexlayoutFloatingWindow.cjs — copy into node_modules and run
 * `npx patch-package flexlayout-react` only when regenerating the .patch file.
 */
console.warn(
  '[patch:flexlayout] deprecated — postinstall uses patch-package. Run: npm run verify:flexlayout-patch',
);
process.exit(0);
