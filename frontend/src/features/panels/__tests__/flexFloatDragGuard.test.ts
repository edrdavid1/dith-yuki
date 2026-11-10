import { describe, it, expect, vi, beforeEach } from 'vitest';
import { Actions, DockLocation, DragDrop, Model, TabNode } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
import {
  cancelFlexLayoutDrag,
  shouldBlockFloatingTabMove,
} from '../flexFloatDragGuard';

function sideBySideModel(): Model {
  const json: IJsonModel = {
    global: {
      tabSetEnableTabStrip: true,
      tabSetEnableDeleteWhenEmpty: true,
      enableEdgeDock: true,
    },
    borders: [],
    layout: {
      type: 'row',
      weight: 100,
      children: [
        {
          type: 'tabset',
          weight: 50,
          children: [{ type: 'tab', name: 'Effect', component: 'effect' }],
        },
        {
          type: 'tabset',
          weight: 50,
          children: [{ type: 'tab', name: 'Color Lab', component: 'colorlab' }],
        },
      ],
    },
  };
  return Model.fromJson(json);
}

function findTab(model: Model, component: string): TabNode {
  let found: TabNode | null = null;
  model.visitNodes((node) => {
    if (node.getType() !== TabNode.TYPE) return;
    const tab = node as TabNode;
    if (tab.getComponent() === component) found = tab;
  });
  if (!found) throw new Error(`missing ${component}`);
  return found;
}

describe('flexFloatDragGuard', () => {
  beforeEach(() => {
    // Ensure no leftover glass drag between cases.
    cancelFlexLayoutDrag();
  });

  it('blocks Layout MOVE_NODE of an already-floating tab (residual undock drag-end)', () => {
    const model = sideBySideModel();
    const effect = findTab(model, 'effect');
    const host = findTab(model, 'colorlab').getParent()!;
    model.doAction(Actions.floatTab(effect.getId()));

    const move = Actions.moveNode(
      effect.getId(),
      host.getId(),
      DockLocation.RIGHT,
      -1,
    );
    expect(shouldBlockFloatingTabMove(model, move)).toBe(true);
  });

  it('allows MOVE_NODE of a still-docked tab', () => {
    const model = sideBySideModel();
    const effect = findTab(model, 'effect');
    const host = findTab(model, 'colorlab').getParent()!;
    const move = Actions.moveNode(
      effect.getId(),
      host.getId(),
      DockLocation.RIGHT,
      -1,
    );
    expect(shouldBlockFloatingTabMove(model, move)).toBe(false);
  });

  it('cancelFlexLayoutDrag is a no-op when idle and clears an active drag', () => {
    expect(DragDrop.instance.isActive()).toBe(false);
    cancelFlexLayoutDrag();
    expect(DragDrop.instance.isActive()).toBe(false);

    const cancelSpy = vi.fn();
    DragDrop.instance.startDrag(
      undefined,
      () => true,
      () => {},
      () => {},
      cancelSpy,
    );
    expect(DragDrop.instance.isActive()).toBe(true);
    cancelFlexLayoutDrag();
    expect(DragDrop.instance.isActive()).toBe(false);
    expect(cancelSpy).toHaveBeenCalled();
  });
});
