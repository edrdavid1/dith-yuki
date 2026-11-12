/**
 * W7: invariant normalizer for the shell layout envelope.
 *
 * Each of layers / effect / colorlab / preview appears exactly once.
 * preview always lives in center. Unknown component ids are removed.
 * Missing known panels are inserted on their default side.
 */

import type { IJsonModel } from 'flexlayout-react';
import { getDefaultFlexLayoutJson } from '../defaults/DefaultLayouts';

export const KNOWN_PANELS = ['layers', 'effect', 'colorlab', 'preview'] as const;
export type KnownPanel = (typeof KNOWN_PANELS)[number];

export type SideJson = IJsonModel;

export interface NormalizedEnvelope {
  left: SideJson;
  center: SideJson;
  right: SideJson;
  changes: string[];
}

const DEFAULT_SIDE: Record<KnownPanel, 'left' | 'center' | 'right'> = {
  layers: 'left',
  effect: 'right',
  colorlab: 'right',
  preview: 'center',
};

const DISPLAY_NAME: Record<KnownPanel, string> = {
  layers: 'Layers',
  effect: 'Effect Settings',
  colorlab: 'Color Lab',
  preview: 'Preview',
};

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

function tabJson(component: KnownPanel, existing?: Record<string, unknown>): Record<string, unknown> {
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

  const chosen = new Map<KnownPanel, TabHit>();
  for (const hit of allHits) {
    if (!(KNOWN_PANELS as readonly string[]).includes(hit.component)) {
      changes.push(`removed unknown:${hit.component}`);
      continue;
    }
    const known = hit.component as KnownPanel;
    // preview must end in center regardless of where it was found.
    if (known === 'preview') {
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
    if (chosen.has(known)) {
      changes.push(`removed duplicate:${known}`);
      continue;
    }
    // Keep on the side it was found (unless empty preference later).
    chosen.set(known, hit);
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
