/**
 * W7: invariant normalizer for the shell layout envelope.
 *
 * Each of layers / effect / colorlab / preview appears exactly once.
 * preview always lives in center. Unknown component ids are removed.
 * Missing known panels are inserted on their default side.
 *
 * Optional panels (gamedev) are kept if present, never auto-injected —
 * workspace sync adds/removes them at runtime.
 */

import type { IJsonModel } from 'flexlayout-react';
import { getDefaultFlexLayoutJson } from '../defaults/DefaultLayouts';

export const KNOWN_PANELS = ['layers', 'effect', 'colorlab', 'preview'] as const;
export type KnownPanel = (typeof KNOWN_PANELS)[number];

/** Workspace-gated panels — allowed in saved layout, not required. */
export const OPTIONAL_PANELS = ['gamedev'] as const;
export type OptionalPanel = (typeof OPTIONAL_PANELS)[number];

export type AllowedPanel = KnownPanel | OptionalPanel;

export type SideJson = IJsonModel;

export interface NormalizedEnvelope {
  left: SideJson;
  center: SideJson;
  right: SideJson;
  changes: string[];
}

const DEFAULT_SIDE: Record<AllowedPanel, 'left' | 'center' | 'right'> = {
  layers: 'left',
  effect: 'right',
  colorlab: 'right',
  preview: 'center',
  gamedev: 'left',
};

const DISPLAY_NAME: Record<AllowedPanel, string> = {
  layers: 'Layers',
  effect: 'Effect Settings',
  colorlab: 'Color Lab',
  preview: 'Preview',
  gamedev: 'Gamedev Settings',
};

function isKnownPanel(id: string): id is KnownPanel {
  return (KNOWN_PANELS as readonly string[]).includes(id);
}

function isOptionalPanel(id: string): id is OptionalPanel {
  return (OPTIONAL_PANELS as readonly string[]).includes(id);
}

function isAllowedPanel(id: string): id is AllowedPanel {
  return isKnownPanel(id) || isOptionalPanel(id);
}

type TabHit = {
  side: 'left' | 'center' | 'right';
  path: number[]; // indices into tabset children arrays — simplified: we rebuild
  component: string;
  tab: Record<string, unknown>;
};

function collectTabs(
  side: 'left' | 'center' | 'right',
  model: SideJson,
): TabHit[] {
  const hits: TabHit[] = [];
  const walk = (node: unknown) => {
    if (!node || typeof node !== 'object') return;
    const n = node as Record<string, unknown>;
    if (n.type === 'tab' && typeof n.component === 'string') {
      hits.push({
        side,
        path: [],
        component: n.component,
        tab: { ...n },
      });
      return;
    }
    const children = n.children;
    if (Array.isArray(children)) {
      for (const child of children) walk(child);
    }
  };
  walk(model.layout);
  return hits;
}

/** Strip all tab nodes; keep structure. Returns empty tabsets. */
function stripTabs(model: SideJson): SideJson {
  const clone = structuredClone(model) as SideJson;
  const walk = (node: unknown): void => {
    if (!node || typeof node !== 'object') return;
    const n = node as Record<string, unknown>;
    if (n.type === 'tabset' && Array.isArray(n.children)) {
      n.children = [];
      return;
    }
    if (Array.isArray(n.children)) {
      for (const child of n.children) walk(child);
    }
  };
  walk(clone.layout);
  return clone;
}

function firstTabSet(model: SideJson): Record<string, unknown> | null {
  let found: Record<string, unknown> | null = null;
  const walk = (node: unknown): void => {
    if (found || !node || typeof node !== 'object') return;
    const n = node as Record<string, unknown>;
    if (n.type === 'tabset') {
      found = n;
      return;
    }
    if (Array.isArray(n.children)) {
      for (const child of n.children) walk(child);
    }
  };
  walk(model.layout);
  return found;
}

function ensureTabSet(model: SideJson): Record<string, unknown> {
  let ts = firstTabSet(model);
  if (ts) return ts;
  // Seed a tabset under layout.
  const layout = model.layout as unknown as Record<string, unknown>;
  const tabset = { type: 'tabset', weight: 100, children: [] as unknown[] };
  layout.children = [tabset];
  ts = tabset;
  return ts;
}

function tabJson(component: AllowedPanel, existing?: Record<string, unknown>): Record<string, unknown> {
  const base = existing ? { ...existing } : {};
  delete base.id;
  delete base.floating;
  return {
    ...base,
    type: 'tab',
    name: (typeof base.name === 'string' && base.name) || DISPLAY_NAME[component],
    component,
    enableFloat: true,
  };
}

/**
 * Normalize three side JSON models. Pure — does not touch Model instances.
 */
export function normalizeLayoutEnvelope(input: {
  left: SideJson;
  center: SideJson;
  right: SideJson;
}): NormalizedEnvelope {
  const changes: string[] = [];
  const sides = {
    left: stripTabs(input.left),
    center: stripTabs(input.center),
    right: stripTabs(input.right),
  };

  const allHits = [
    ...collectTabs('left', input.left),
    ...collectTabs('center', input.center),
    ...collectTabs('right', input.right),
  ];

  const chosen = new Map<AllowedPanel, TabHit>();
  for (const hit of allHits) {
    if (!isAllowedPanel(hit.component)) {
      changes.push(`removed unknown:${hit.component}`);
      continue;
    }
    const panel = hit.component;
    // preview must end in center regardless of where it was found.
    if (panel === 'preview') {
      if (!chosen.has('preview')) {
        chosen.set('preview', { ...hit, side: 'center' });
        if (hit.side !== 'center') {
          changes.push('moved preview→center');
        }
      } else {
        changes.push('removed duplicate:preview');
      }
      continue;
    }
    // Optional / sidebar panels must not live in the center host.
    if (hit.side === 'center') {
      changes.push(`moved ${panel}→${DEFAULT_SIDE[panel]}`);
      if (chosen.has(panel)) {
        changes.push(`removed duplicate:${panel}`);
        continue;
      }
      chosen.set(panel, { ...hit, side: DEFAULT_SIDE[panel] });
      continue;
    }
    if (chosen.has(panel)) {
      changes.push(`removed duplicate:${panel}`);
      continue;
    }
    // Keep on the side it was found.
    chosen.set(panel, hit);
  }

  for (const panel of KNOWN_PANELS) {
    if (chosen.has(panel)) continue;
    const side = DEFAULT_SIDE[panel];
    chosen.set(panel, {
      side,
      path: [],
      component: panel,
      tab: tabJson(panel),
    });
    changes.push(`injected ${panel}→${side}`);
  }

  for (const [panel, hit] of chosen) {
    const side = panel === 'preview' ? 'center' : hit.side;
    // If a non-preview was found on the wrong host for defaults we still keep
    // its found side (user may have moved it) — except preview.
    const targetSide = panel === 'preview' ? 'center' : side;
    const model = sides[targetSide];
    const ts = ensureTabSet(model);
    const children = (ts.children as unknown[]) ?? [];
    children.push(tabJson(panel, hit.tab));
    ts.children = children;
  }

  // Ensure empty sides still have valid structure.
  for (const side of ['left', 'center', 'right'] as const) {
    if (!sides[side].layout) {
      sides[side] = getDefaultFlexLayoutJson(side);
      changes.push(`reset empty:${side}`);
    }
  }

  return { left: sides.left, center: sides.center, right: sides.right, changes };
}
