import { describe, it, expect } from 'vitest';
import { getDefaultFlexLayoutJson } from '../../defaults/DefaultLayouts';
import { normalizeLayoutEnvelope } from '../layoutNormalizer';
import type { IJsonModel } from 'flexlayout-react';

function sideWithTabs(
  tabs: Array<{ component: string; name?: string }>,
): IJsonModel {
  return {
    global: {},
    borders: [],
    layout: {
      type: 'row',
      weight: 100,
      children: [
        {
          type: 'tabset',
          weight: 100,
          children: tabs.map((t) => ({
            type: 'tab',
            name: t.name ?? t.component,
            component: t.component,
          })),
        },
      ],
    },
  };
}

function componentsIn(model: IJsonModel): string[] {
  const out: string[] = [];
  const walk = (n: unknown) => {
    if (!n || typeof n !== 'object') return;
    const o = n as Record<string, unknown>;
    if (o.type === 'tab' && typeof o.component === 'string') out.push(o.component);
    if (Array.isArray(o.children)) o.children.forEach(walk);
  };
  walk(model.layout);
  return out;
}

describe('normalizeLayoutEnvelope (W7)', () => {
  it('injects missing colorlab on the right', () => {
    const result = normalizeLayoutEnvelope({
      left: sideWithTabs([{ component: 'layers' }]),
      center: sideWithTabs([{ component: 'preview' }]),
      right: sideWithTabs([{ component: 'effect' }]),
    });
    expect(componentsIn(result.right)).toContain('colorlab');
    expect(result.changes.some((c) => c.includes('injected colorlab'))).toBe(true);
  });

  it('removes duplicate layers keeping one', () => {
    const result = normalizeLayoutEnvelope({
      left: sideWithTabs([{ component: 'layers' }]),
      center: sideWithTabs([{ component: 'preview' }]),
      right: sideWithTabs([
        { component: 'effect' },
        { component: 'colorlab' },
        { component: 'layers' },
      ]),
    });
    const all = [
      ...componentsIn(result.left),
      ...componentsIn(result.center),
      ...componentsIn(result.right),
    ];
    expect(all.filter((c) => c === 'layers')).toHaveLength(1);
    expect(result.changes.some((c) => c.includes('duplicate:layers'))).toBe(true);
  });

  it('removes unknown component ids', () => {
    const result = normalizeLayoutEnvelope({
      left: sideWithTabs([
        { component: 'layers' },
        { component: 'mystery-panel' },
      ]),
      center: sideWithTabs([{ component: 'preview' }]),
      right: sideWithTabs([
        { component: 'effect' },
        { component: 'colorlab' },
      ]),
    });
    const all = [
      ...componentsIn(result.left),
      ...componentsIn(result.center),
      ...componentsIn(result.right),
    ];
    expect(all).not.toContain('mystery-panel');
    expect(result.changes.some((c) => c.includes('unknown:mystery-panel'))).toBe(
      true,
    );
  });

  it('forces preview into center', () => {
    const result = normalizeLayoutEnvelope({
      left: sideWithTabs([{ component: 'layers' }, { component: 'preview' }]),
      center: getDefaultFlexLayoutJson('center'), // will be stripped/rebuilt
      right: sideWithTabs([
        { component: 'effect' },
        { component: 'colorlab' },
      ]),
    });
    expect(componentsIn(result.center)).toEqual(['preview']);
    expect(componentsIn(result.left)).not.toContain('preview');
  });
});
