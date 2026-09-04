/**
 * Default FlexLayout JSON — per side.
 *
 * Produces an IJsonModel compatible with Model.fromJson().
 * Format: { global?, borders?, layout: IJsonRowNode }
 *
 * B3/B4a/B4b:
 *   left  — Layers tab
 *   right — Effect Settings + Color Lab (vertical stack via applyAppChromePolicy)
 */

import type { IJsonModel } from 'flexlayout-react';

export type FlexSide = 'left' | 'right';

// ─── Shared global config ─────────────────────────────────────────────────────

const FLEX_GLOBAL = {
  tabEnableRename: false,
  tabEnableClose: false,
  tabSetEnableMaximize: false,
  // Tab strip is our WindowTitlebar chrome (styled via onRenderTab); enables drag.
  tabSetEnableTabStrip: true,
  // Single tab fills the strip like a classic window titlebar.
  tabSetEnableSingleTabStretch: true,
  // FlexLayout OS popout via window.open (not Tauri WebviewWindow).
  tabEnableFloat: true,
  // Hit target only — layout size 0 so stacked panel borders share one line
  // (see flexlayout-theme.css splitter_horz negative margin).
  splitterSize: 0,
  splitterExtra: 4,
} as const;

// ─── Default layout JSON ──────────────────────────────────────────────────────

/**
 * Returns the IJsonModel object for the given side's default layout.
 *
 * left  → Layers
 * right → Effect Settings + Color Lab (peer tabs; normalized to vertical stack on load)
 */
export function getDefaultFlexLayoutJson(side: FlexSide = 'left'): IJsonModel {
  const tabs =
    side === 'left'
      ? [{ type: 'tab', name: 'Layers', component: 'layers' }]
      : [
          { type: 'tab', name: 'Effect Settings', component: 'effect' },
          { type: 'tab', name: 'Color Lab', component: 'colorlab' },
        ];

  return {
    global: FLEX_GLOBAL,
    borders: [],
    layout: {
      type: 'row',
      weight: 100,
      children: [
        {
          type: 'tabset',
          weight: 100,
          children: tabs,
        },
      ],
    },
  };
}

/**
 * Returns the default layout as a JSON string for the given side.
 */
export function getDefaultFlexLayout(side: FlexSide = 'left'): string {
  return JSON.stringify(getDefaultFlexLayoutJson(side));
}

// ─── Toast messages ───────────────────────────────────────────────────────────

export const LAYOUT_TOAST_MESSAGES = {
  RESET_TO_DEFAULT:  'Layout reset to default',
  MIGRATION_V2_TO_V3: 'Layout updated — please reconfigure your panel positions',
  CORRUPT_RECOVERED: 'Layout file was corrupted; recovered to default',
  /** Existing flexlayout_* without colorlab — tab injected, rest of layout kept. */
  COLORLAB_ADDED: 'Color Lab added to the panel',
} as const;

// ─── Validation helpers ───────────────────────────────────────────────────────

/**
 * Very light structural check — is this a plausible IJsonModel?
 * Full validation is done by Model.fromJson() itself.
 */
export function isValidFlexLayoutJson(value: unknown): value is IJsonModel {
  if (typeof value !== 'object' || value === null) return false;
  const obj = value as Record<string, unknown>;
  return (
    typeof obj['layout'] === 'object' &&
    obj['layout'] !== null &&
    (obj['layout'] as Record<string, unknown>)['type'] === 'row'
  );
}

/**
 * Parse a JSON string into an IJsonModel, returning null on failure.
 */
export function parseFlexLayoutJson(jsonString: string): IJsonModel | null {
  try {
    const parsed: unknown = JSON.parse(jsonString);
    if (isValidFlexLayoutJson(parsed)) return parsed;
    console.warn('[DefaultLayouts] Parsed JSON failed schema check; using default.');
    return null;
  } catch (err) {
    console.warn('[DefaultLayouts] JSON.parse failed:', err);
    return null;
  }
}

/**
 * Returns a safe IJsonModel: parses the string, falls back to the side default.
 */
export function getSafeFlexLayout(jsonString: string, side: FlexSide = 'left'): IJsonModel {
  return parseFlexLayoutJson(jsonString) ?? getDefaultFlexLayoutJson(side);
}

/**
 * Check whether a given IJsonModel is structurally identical to the default for a side.
 */
export function isDefaultLayout(value: unknown, side: FlexSide = 'left'): boolean {
  if (!isValidFlexLayoutJson(value)) return false;
  return JSON.stringify(value) === JSON.stringify(getDefaultFlexLayoutJson(side));
}
