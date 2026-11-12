/**
 * W6: DEV invariant checks for flex-popout registry ↔ live windows ↔ model.
 */

import { getAllWebviewWindows } from '@tauri-apps/api/webviewWindow';
import type { Model } from 'flexlayout-react';
import { listFloatingFlexComponents } from '../../contexts/layoutModelOps';
import { listFlexPopoutRegistryEntries } from './flexPopoutRegistry';

const FLEX_POPOUT_PREFIX = 'flex-popout-';

export type FlexPopoutInvariantViolation = {
  kind:
    | 'registry-label-missing-window'
    | 'floating-tab-missing-registry';
  detail: string;
};

/**
 * Every registry label must name a live `flex-popout-*` window;
 * every floating tab should have a registry entry.
 */
export async function checkFlexPopoutInvariants(models: {
  left: Model | null;
  center: Model | null;
  right: Model | null;
}): Promise<FlexPopoutInvariantViolation[]> {
  const violations: FlexPopoutInvariantViolation[] = [];
  let wins: Awaited<ReturnType<typeof getAllWebviewWindows>> = [];
  try {
    wins = await getAllWebviewWindows();
  } catch (err) {
    console.error('[flex-dock] invariant: list windows failed', err);
    return violations;
  }
  const live = new Set(
    wins.filter((w) => w.label.startsWith(FLEX_POPOUT_PREFIX)).map((w) => w.label),
  );

  for (const { panelId, label } of listFlexPopoutRegistryEntries()) {
    if (!live.has(label)) {
      violations.push({
        kind: 'registry-label-missing-window',
        detail: `${panelId} → ${label} (no live window)`,
      });
    }
  }

  const floating = [
    ...listFloatingFlexComponents(models.left),
    ...listFloatingFlexComponents(models.center),
    ...listFloatingFlexComponents(models.right),
  ];
  const registered = new Set(
    listFlexPopoutRegistryEntries().map((e) => e.panelId),
  );
  for (const panelId of floating) {
    if (!registered.has(panelId)) {
      violations.push({
        kind: 'floating-tab-missing-registry',
        detail: panelId,
      });
    }
  }

  return violations;
}

/** Log violations in DEV; no-op in production builds. */
export async function runFlexPopoutInvariantCheck(models: {
  left: Model | null;
  center: Model | null;
  right: Model | null;
}): Promise<void> {
  if (!import.meta.env.DEV) return;
  const violations = await checkFlexPopoutInvariants(models);
  for (const v of violations) {
    console.warn('[flex-dock] invariant', v.kind, v.detail);
  }
}
