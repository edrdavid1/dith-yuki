import { describe, it, expect } from 'vitest';
import { Actions, DockLocation, Model, TabNode, TabSetNode } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
import {
  DOCK_TABSET_MIN_WIDTH,
  SIDEBAR_EXPANDED_MIN_WIDTH,
} from '../../defaults/DefaultLayouts';
import {
  maxDockedHorizontalSpan,
  sidebarWidthForHorizontalSpan,
} from '../../features/panels/horizontalDock';

function modelFrom(children: IJsonModel['layout']['children']): Model {
  return Model.fromJson({
    global: {},
    borders: [],
    layout: { type: 'row', weight: 100, children },
  });
}

function findTab(model: Model, component: string): TabNode {
  let found: TabNode | null = null;
  model.visitNodes((node) => {
    if (node.getType() !== TabNode.TYPE) return;
    const t = node as TabNode;
    if (t.getComponent() === component) found = t;
  });
  if (!found) throw new Error(`missing ${component}`);
  return found;
}

describe('maxDockedHorizontalSpan', () => {
  it('is 1 for a single tabset', () => {
    const model = modelFrom([
      {
        type: 'tabset',
        weight: 100,
        children: [{ type: 'tab', name: 'Layers', component: 'layers' }],
      },
    ]);
    expect(maxDockedHorizontalSpan(model)).toBe(1);
  });

  it('counts side-by-side tabsets in a horizontal row', () => {
    const model = modelFrom([
      {
        type: 'tabset',
        weight: 50,
        children: [{ type: 'tab', name: 'Effect Settings', component: 'effect' }],
      },
      {
        type: 'tabset',
        weight: 50,
        children: [{ type: 'tab', name: 'Color Lab', component: 'colorlab' }],
      },
    ]);
    expect(maxDockedHorizontalSpan(model)).toBe(2);
  });

  it('does not count a vertical stack as multiple columns', () => {
    const model = Model.fromJson({
      global: { rootOrientationVertical: true },
      borders: [],
      layout: {
        type: 'row',
        weight: 100,
        children: [
          {
            type: 'tabset',
            weight: 50,
            children: [{ type: 'tab', name: 'Effect Settings', component: 'effect' }],
          },
          {
            type: 'tabset',
            weight: 50,
            children: [{ type: 'tab', name: 'Color Lab', component: 'colorlab' }],
          },
        ],
      },
    });
    // Root is vertical → stacked; horizontal span stays 1.
    expect(maxDockedHorizontalSpan(model)).toBe(1);
  });
});

describe('sidebarWidthForHorizontalSpan', () => {
  it('keeps preferred width when already wide enough', () => {
    expect(sidebarWidthForHorizontalSpan(1, 332)).toBe(332);
  });

  it('grows to fit side-by-side min widths', () => {
    expect(sidebarWidthForHorizontalSpan(2, 332)).toBe(2 * DOCK_TABSET_MIN_WIDTH);
  });

  it('never goes below the expanded sidebar floor', () => {
    expect(sidebarWidthForHorizontalSpan(1, 100)).toBe(SIDEBAR_EXPANDED_MIN_WIDTH);
  });
});

describe('side-by-side move preserves horizontal row', () => {
  it('left drop creates two tabsets side by side', () => {
    const model = modelFrom([
      {
        type: 'tabset',
        weight: 100,
        children: [
          { type: 'tab', name: 'Effect Settings', component: 'effect' },
          { type: 'tab', name: 'Color Lab', component: 'colorlab' },
        ],
      },
    ]);
    let firstTs: TabSetNode | null = null;
    model.visitNodes((node) => {
      if (!firstTs && node.getType() === TabSetNode.TYPE) firstTs = node as TabSetNode;
    });
    model.doAction(
      Actions.moveNode(findTab(model, 'colorlab').getId(), firstTs!.getId(), DockLocation.LEFT, -1),
    );
    expect(maxDockedHorizontalSpan(model)).toBe(2);
  });
});
