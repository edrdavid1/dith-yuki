import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { Actions, DockLocation, DragDrop, Model, TabNode } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
import { EXIT_SLACK_PX } from '../gestureConfig';
import {
  cancelFlexLayoutDrag,
  filterLayoutAction,
  shouldBlockFloatingTabMove,
  TitlebarGestureController,
} from '../titlebarGestureController';

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

function makeColumn(): HTMLDivElement {
  const column = document.createElement('div');
  document.body.appendChild(column);
  column.getBoundingClientRect = () =>
    ({
      left: 100,
      right: 400,
      top: 0,
      bottom: 600,
      width: 300,
      height: 600,
      x: 100,
      y: 0,
      toJSON: () => ({}),
    }) as DOMRect;
  return column;
}

describe('TitlebarGestureController', () => {
  let column: HTMLDivElement;

  beforeEach(() => {
    cancelFlexLayoutDrag();
    column = makeColumn();
  });

  afterEach(() => {
    column.remove();
  });

  it('movement inside the column does not undock', () => {
    const onUndock = vi.fn();
    const ctrl = new TitlebarGestureController({
      getColumnEl: () => column,
      getMode: () => 'sidebar',
      onUndock,
    });
    ctrl.onTitlebarPointerDown({
      button: 0,
      clientX: 200,
      clientY: 100,
      target: column,
    });
    document.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 220, clientY: 100 }),
    );
    document.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 250, clientY: 120 }),
    );
    expect(onUndock).not.toHaveBeenCalled();
    ctrl.dispose();
  });

  it('leaving past EXIT_SLACK triggers exactly one undock after cancelFlexLayoutDrag', () => {
    const onUndock = vi.fn();
    const cancelSpy = vi.fn();
    DragDrop.instance.startDrag(
      undefined,
      () => true,
      () => {},
      () => {},
      cancelSpy,
    );
    expect(DragDrop.instance.isActive()).toBe(true);

    const ctrl = new TitlebarGestureController({
      getColumnEl: () => column,
      getMode: () => 'sidebar',
      onUndock,
    });
    ctrl.onTitlebarPointerDown({
      button: 0,
      clientX: 200,
      clientY: 100,
      target: column,
    });
    // Arm
    document.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 210, clientY: 100 }),
    );
    // Inside slack — no undock
    document.dispatchEvent(
      new PointerEvent('pointermove', {
        clientX: 100 - 30,
        clientY: 100,
      }),
    );
    expect(onUndock).not.toHaveBeenCalled();

    // Past left EXIT_SLACK
    document.dispatchEvent(
      new PointerEvent('pointermove', {
        clientX: 100 - (EXIT_SLACK_PX + 6),
        clientY: 100,
      }),
    );
    expect(onUndock).toHaveBeenCalledTimes(1);
    expect(cancelSpy).toHaveBeenCalled();
    expect(DragDrop.instance.isActive()).toBe(false);

    // Further moves / pointerup must not undock again
    document.dispatchEvent(
      new PointerEvent('pointermove', {
        clientX: 100 - (EXIT_SLACK_PX + 40),
        clientY: 100,
      }),
    );
    document.dispatchEvent(new PointerEvent('pointerup'));
    expect(onUndock).toHaveBeenCalledTimes(1);
    ctrl.dispose();
  });

  it('mouseup after undock cannot MOVE_NODE a live floating popout', () => {
    const model = sideBySideModel();
    const effect = findTab(model, 'effect');
    const host = findTab(model, 'colorlab').getParent()!;
    const onUndock = vi.fn(() => {
      model.doAction(Actions.floatTab(effect.getId()));
    });

    const ctrl = new TitlebarGestureController({
      getColumnEl: () => column,
      getMode: () => 'sidebar',
      onUndock,
    });
    ctrl.onTitlebarPointerDown({
      button: 0,
      clientX: 200,
      clientY: 100,
      target: column,
    });
    document.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 210, clientY: 100 }),
    );
    document.dispatchEvent(
      new PointerEvent('pointermove', {
        clientX: 100 - (EXIT_SLACK_PX + 6),
        clientY: 100,
      }),
    );
    expect(onUndock).toHaveBeenCalledTimes(1);
    expect(effect.isFloating()).toBe(true);

    // Residual edge-dock mouseup → MOVE_NODE of the floated tab must be rejected.
    const move = Actions.moveNode(
      effect.getId(),
      host.getId(),
      DockLocation.RIGHT,
      -1,
    );
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => {});
    expect(filterLayoutAction(model, move)).toBeUndefined();
    expect(debug).toHaveBeenCalledWith(
      '[flex-dock] reject MOVE_NODE for floating tab',
      effect.getId(),
    );
    debug.mockRestore();
    document.dispatchEvent(new PointerEvent('pointerup'));
    ctrl.dispose();
  });
});

describe('filterLayoutAction / cancelFlexLayoutDrag', () => {
  beforeEach(() => {
    cancelFlexLayoutDrag();
  });

  it('blocks Layout MOVE_NODE of an already-floating tab', () => {
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
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => {});
    expect(filterLayoutAction(model, move)).toBeUndefined();
    expect(debug).toHaveBeenCalled();
    debug.mockRestore();
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
    expect(filterLayoutAction(model, move)).toBe(move);
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
