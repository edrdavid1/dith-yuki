import { describe, it, expect } from 'vitest';
import { Actions, DockLocation, Model, TabNode, TabSetNode } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
// Rect is used by FlexLayout internals for _layout; not part of the public export.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const { Rect } = require('flexlayout-react/lib/Rect.js') as {
  Rect: new (x: number, y: number, w: number, h: number) => {
    x: number;
    y: number;
    width: number;
    height: number;
  };
};
import { DOCK_TABSET_MIN_HEIGHT } from '../../defaults/DefaultLayouts';
import { rebalanceFloatingTabsets } from '../LayoutContext';

const COL_H = 600;
const COL_W = 280;

/** Minimal vertical stack mirroring the right sidebar (Effect + Color Lab). */
function stackedModel(): Model {
  const json: IJsonModel = {
    global: {
      tabSetEnableTabStrip: true,
      tabSetEnableDeleteWhenEmpty: true,
      splitterSize: 0,
      tabSetMinHeight: DOCK_TABSET_MIN_HEIGHT,
      tabSetTabStripHeight: DOCK_TABSET_MIN_HEIGHT,
      rootOrientationVertical: true,
    },
    borders: [],
    layout: {
      type: 'row',
      weight: 100,
      children: [
        {
          type: 'tabset',
          weight: 100,
          children: [
            { type: 'tab', name: 'Effect Settings', component: 'effect' },
            { type: 'tab', name: 'Color Lab', component: 'colorlab' },
          ],
        },
      ],
    },
  };
  const model = Model.fromJson(json);
  const tabsetIds: string[] = [];
  model.visitNodes((node) => {
    if (node.getType() === TabSetNode.TYPE) tabsetIds.push(node.getId());
  });
  for (const tsId of tabsetIds) {
    for (;;) {
      const ts = model.getNodeById(tsId);
      if (!ts || ts.getType() !== TabSetNode.TYPE) break;
      const docked = (ts as TabSetNode)
        .getChildren()
        .filter((c) => c.getType() === TabNode.TYPE && !(c as TabNode).isFloating());
      if (docked.length <= 1) break;
      model.doAction(
        Actions.moveNode(docked[docked.length - 1]!.getId(), tsId, DockLocation.BOTTOM, -1),
      );
    }
  }
  rebalanceFloatingTabsets(model);
  return model;
}

function tabsets(model: Model): TabSetNode[] {
  const out: TabSetNode[] = [];
  model.visitNodes((node) => {
    if (node.getType() === TabSetNode.TYPE) out.push(node as TabSetNode);
  });
  return out;
}

function findTab(model: Model, component: string): TabNode {
  let found: TabNode | null = null;
  model.visitNodes((node) => {
    if (node.getType() !== TabNode.TYPE) return;
    const tab = node as TabNode;
    if (tab.getComponent() === component) found = tab;
  });
  if (!found) throw new Error(`tab ${component} missing`);
  return found;
}

function layout(model: Model): void {
  (model as unknown as { _layout: (r: unknown, m: unknown) => void })._layout(
    new Rect(0, 0, COL_W, COL_H),
    { headerBarSize: 0, tabBarSize: DOCK_TABSET_MIN_HEIGHT, borderBarSize: 0 },
  );
}

function rectOf(ts: TabSetNode): { y: number; h: number } {
  const r = ts.getRect();
  return { y: r.y, h: r.height };
}

function floatAndRebalance(model: Model, component: string): void {
  model.doAction(Actions.floatTab(findTab(model, component).getId()));
  rebalanceFloatingTabsets(model);
}

function dockAndRebalance(model: Model, component: string): void {
  model.doAction(Actions.unFloatTab(findTab(model, component).getId()));
  // Same as floatPanel/dockPanel — normalize peers back into a vertical stack.
  const tabsetIds = tabsets(model).map((ts) => ts.getId());
  for (const tsId of tabsetIds) {
    for (;;) {
      const ts = model.getNodeById(tsId);
      if (!ts || ts.getType() !== TabSetNode.TYPE) break;
      const docked = (ts as TabSetNode)
        .getChildren()
        .filter((c) => c.getType() === TabNode.TYPE && !(c as TabNode).isFloating());
      if (docked.length <= 1) break;
      model.doAction(
        Actions.moveNode(docked[docked.length - 1]!.getId(), tsId, DockLocation.BOTTOM, -1),
      );
    }
  }
  rebalanceFloatingTabsets(model);
}

describe('rebalanceFloatingTabsets', () => {
  it('parks a floated tab into the docked sibling and deletes the empty host', () => {
    const model = stackedModel();
    expect(tabsets(model)).toHaveLength(2);

    floatAndRebalance(model, 'effect');

    // One tabset left — floating Effect parked beside docked Color Lab.
    expect(tabsets(model)).toHaveLength(1);
    const host = tabsets(model)[0]!;
    const kids = host.getChildren() as TabNode[];
    expect(kids.map((t) => `${t.getComponent()}${t.isFloating() ? '*' : ''}`).sort()).toEqual([
      'colorlab',
      'effect*',
    ]);
    expect(host.getHeight()).toBeUndefined();
    expect(host.getMinHeight()).toBe(DOCK_TABSET_MIN_HEIGHT);
    expect(host.isEnableTabStrip()).toBe(true);
    // Selected tab must be the docked one (not the floating park).
    const selected = host.getSelectedNode() as TabNode;
    expect(selected.isFloating()).toBe(false);
  });

  it('undocking the top panel leaves one tabset at full column height (no top strip)', () => {
    const model = stackedModel();
    floatAndRebalance(model, 'effect');
    layout(model);

    expect(tabsets(model)).toHaveLength(1);
    expect(rectOf(tabsets(model)[0]!)).toEqual({ y: 0, h: COL_H });
  });

  it('undocking the bottom panel leaves one tabset at full column height (no bottom strip)', () => {
    const model = stackedModel();
    floatAndRebalance(model, 'colorlab');
    layout(model);

    expect(tabsets(model)).toHaveLength(1);
    expect(rectOf(tabsets(model)[0]!)).toEqual({ y: 0, h: COL_H });
  });

  it('recalculates from scratch across float both / redock one / redock other', () => {
    const model = stackedModel();

    floatAndRebalance(model, 'effect');
    floatAndRebalance(model, 'colorlab');
    layout(model);
    // Both floating — single floating-only host collapsed to height 0.
    expect(tabsets(model)).toHaveLength(1);
    expect(tabsets(model)[0]!.getHeight()).toBe(0);
    expect(rectOf(tabsets(model)[0]!).h).toBe(0);

    dockAndRebalance(model, 'effect');
    layout(model);
    {
      const sets = tabsets(model);
      const docked = sets.filter((ts) =>
        ts.getChildren().some((c) => c.getType() === TabNode.TYPE && !(c as TabNode).isFloating()),
      );
      expect(docked).toHaveLength(1);
      expect(rectOf(docked[0]!)).toEqual({ y: 0, h: COL_H });
    }

    dockAndRebalance(model, 'colorlab');
    layout(model);
    const both = tabsets(model);
    expect(both).toHaveLength(2);
    expect(both[0]!.getHeight()).toBeUndefined();
    expect(both[1]!.getHeight()).toBeUndefined();
    expect(rectOf(both[0]!).h + rectOf(both[1]!).h).toBe(COL_H);
  });

  it('restores a vertical stack when the floated tab docks back', () => {
    const model = stackedModel();
    floatAndRebalance(model, 'effect');
    dockAndRebalance(model, 'effect');
    layout(model);

    expect(tabsets(model)).toHaveLength(2);
    for (const ts of tabsets(model)) {
      expect(ts.getHeight()).toBeUndefined();
      expect(ts.getMinHeight()).toBe(DOCK_TABSET_MIN_HEIGHT);
      expect(ts.isEnableTabStrip()).toBe(true);
    }
    expect(rectOf(tabsets(model)[0]!).h + rectOf(tabsets(model)[1]!).h).toBe(COL_H);
  });
});
