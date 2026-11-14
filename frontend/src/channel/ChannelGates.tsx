import { lazy, Suspense, useState } from 'react';
import { IS_DIRECT } from './flags';

/** Compile-time gate so store bundles drop the license dialog module. */
const LicenseDialog = IS_DIRECT
  ? lazy(() => import('./license/LicenseDialog'))
  : null;

/**
 * Direct-only overlays. Open the license dialog from a menu later; the
 * `IS_DIRECT ? lazy(...)` form is what Vite uses to strip store bundles.
 */
export function ChannelGates() {
  const [licenseOpen, setLicenseOpen] = useState(false);

  if (!IS_DIRECT || !LicenseDialog) return null;

  return (
    <Suspense fallback={null}>
      <LicenseDialog isOpen={licenseOpen} onClose={() => setLicenseOpen(false)} />
    </Suspense>
  );
}
