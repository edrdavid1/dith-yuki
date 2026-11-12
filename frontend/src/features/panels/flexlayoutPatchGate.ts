/**
 * W3: runtime detection of the flexlayout-react Tauri patch marker.
 * Marker is injected by the Vite plugin after verify:flexlayout-patch.
 */

import { marker as injectedMarker, ok as injectedOk } from 'virtual:flexlayout-patch';

export type FlexPatchMarker = {
  version: number;
  features: string[];
};

const EXPECTED_VERSION = 1;

export function getFlexPatchMarker(): FlexPatchMarker | null {
  if (!injectedOk || !injectedMarker) return null;
  if (injectedMarker.version !== EXPECTED_VERSION) return null;
  if (
    !Array.isArray(injectedMarker.features) ||
    !injectedMarker.features.includes('named-popout')
  ) {
    return null;
  }
  return injectedMarker;
}

export function isFlexPatchApplied(): boolean {
  return getFlexPatchMarker() != null;
}

/** Undock / float UI — false when the patch is missing. */
export function floatUiAllowed(): boolean {
  return isFlexPatchApplied();
}
