import { describe, expect, it } from 'vitest';
import { Actions, Model, TabNode } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
import { DOCK_TABSET_MIN_HEIGHT } from '../../defaults/DefaultLayouts';
import {
  dockAllFloating,
  listDockedFlexComponents,
  listFloatingFlexComponents,
} from '../LayoutContext';

function floatingTripleModel(): Model {
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
            {
              type: 'tab',
              name: 'Layers',
              component: 'layers',
              floating: true,
            },
            {
              type: 'tab',
              name: 'Effect Settings',
              component: 'effect',
              floating: true,
            },
            {
              type: 'tab',
              name: 'Color Lab',
              component: 'colorlab',
              floating: true,
            },
          ],
        },
      ],
    },
  };
  return Model.fromJson(json);
}

describe('dockAllFloating (W2)', () => {
  it('JSON with three floating tabs loads to 0 floating and 3 docked', () => {
    const model = floatingTripleModel();
    expect(listFloatingFlexComponents(model).sort()).toEqual([
      'colorlab',
      'effect',
      'layers',
    ]);

    const n = dockAllFloating(model);
    expect(n).toBe(3);
    expect(listFloatingFlexComponents(model)).toEqual([]);
    expect(listDockedFlexComponents(model).sort()).toEqual([
      'colorlab',
      'effect',
      'layers',
    ]);

    // Tabs are no longer floating.
    model.visitNodes((node) => {
      if (node.getType() !== TabNode.TYPE) return;
      expect((node as TabNode).isFloating()).toBe(false);
    });
  });

  it('is a no-op when nothing is floating', () => {
    const model = floatingTripleModel();
    dockAllFloating(model);
    expect(dockAllFloating(model)).toBe(0);
  });

  it('docks a single floating tab after floatTab', () => {
    const json: IJsonModel = {
      global: {
        tabSetEnableTabStrip: true,
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
            children: [{ type: 'tab', name: 'Layers', component: 'layers' }],
          },
        ],
      },
    };
    const model = Model.fromJson(json);
    let tabId: string | null = null;
    model.visitNodes((node) => {
      if (node.getType() === TabNode.TYPE) tabId = node.getId();
    });
    model.doAction(Actions.floatTab(tabId!));
    expect(listFloatingFlexComponents(model)).toEqual(['layers']);
    expect(dockAllFloating(model)).toBe(1);
    expect(listFloatingFlexComponents(model)).toEqual([]);
  });
});
