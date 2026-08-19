import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  clampSplitRatio,
  panelStackFlex,
  resolvePanelDragMode,
} from '../../features/panels/panelDragMode';
import { sidebarEffectiveWidth } from '../../features/panels/DockedSidebar';
import {
  builtinWorkspacePresets,
  deleteWorkspacePreset,
  listWorkspacePresets,
  snapshotFromPanelState,
} from '../../features/panels/workspacePresets';
import { migrateShellPrefs } from '../shell/ShellContext';
import { selectVisibleDocked } from '../slices/panelsSlice';
import type { PanelStateSnapshot } from '../../types/panels';

describe('8.2 dual-sidebar scenarios (automated)', () => {
  const defaultSnap: PanelStateSnapshot = {
    panels: [
      {
        id: 'layers',
        docked: true,
        visible: true,
        window_label: null,
        saved_bounds: null,
        dock_side: 'left',
      },
      {
        id: 'effect',
        docked: true,
        visible: true,
        window_label: null,
        saved_bounds: null,
        dock_side: 'right',
      },
      {
        id: 'colorlab',
        docked: true,
        visible: true,
        window_label: null,
        saved_bounds: null,
        dock_side: 'right',
      },
    ],
    left_order: ['layers'],
    right_order: ['effect', 'colorlab'],
  };

  it('Default: Layers left, Effect+Color Lab right → dual widths', () => {
    const left = selectVisibleDocked(
      defaultSnap.panels,
      defaultSnap.left_order,
      defaultSnap.right_order,
      'left'
    );
    const right = selectVisibleDocked(
      defaultSnap.panels,
      defaultSnap.left_order,
      defaultSnap.right_order,
      'right'
    );
    expect(left).toEqual(['layers']);
    expect(right).toEqual(['effect', 'colorlab']);
    expect(sidebarEffectiveWidth(left.length, false, 332)).toBe(332);
    expect(sidebarEffectiveWidth(right.length, false, 332)).toBe(332);
  });

  it('Single-stack all right → left width 0', () => {
    const snap: PanelStateSnapshot = {
      panels: defaultSnap.panels.map((p) => ({ ...p, dock_side: 'right' as const })),
      left_order: [],
      right_order: ['layers', 'effect', 'colorlab'],
    };
    expect(
      selectVisibleDocked(snap.panels, snap.left_order, snap.right_order, 'left')
    ).toEqual([]);
    expect(
      selectVisibleDocked(snap.panels, snap.left_order, snap.right_order, 'right')
    ).toHaveLength(3);
    expect(sidebarEffectiveWidth(0, false, 332)).toBe(0);
    expect(sidebarEffectiveWidth(3, false, 332)).toBe(332);
  });

  it('Single-stack all left → right width 0', () => {
    expect(sidebarEffectiveWidth(3, false, 300)).toBe(300);
    expect(sidebarEffectiveWidth(0, false, 300)).toBe(0);
  });

  it('From single-stack, one panel on empty side restores dual widths', () => {
    const leftN = 1;
    const rightN = 2;
    expect(sidebarEffectiveWidth(leftN, false, 332) > 0).toBe(true);
    expect(sidebarEffectiveWidth(rightN, false, 332) > 0).toBe(true);
  });

  it('Collapse left only does not change right effective width', () => {
    expect(sidebarEffectiveWidth(1, true, 332)).toBe(40);
    expect(sidebarEffectiveWidth(2, false, 332)).toBe(332);
  });

  it('Hide last panel on a side closes that column', () => {
    const entities = defaultSnap.panels.map((p) =>
      p.id === 'layers' ? { ...p, visible: false } : p
    );
    const left = selectVisibleDocked(
      entities,
      defaultSnap.left_order,
      defaultSnap.right_order,
      'left'
    );
    expect(left).toEqual([]);
    expect(sidebarEffectiveWidth(left.length, false, 332)).toBe(0);
  });

  it('Both floating → canvas full width (both columns 0)', () => {
    const entities = defaultSnap.panels.map((p) => ({
      ...p,
      docked: false,
      dock_side: null,
    }));
    const left = selectVisibleDocked(entities, [], [], 'left');
    const right = selectVisibleDocked(entities, [], [], 'right');
    expect(sidebarEffectiveWidth(left.length, false, 332)).toBe(0);
    expect(sidebarEffectiveWidth(right.length, false, 332)).toBe(0);
  });

  it('Legacy sidebarSide=left migrates shell stack prefs to left', () => {
    const migrated = migrateShellPrefs({
      sidebarSide: 'left',
      sidebarWidth: 400,
      sidebarCollapsed: true,
    });
    expect(migrated.leftSidebar).toEqual({ width: 400, collapsed: true });
    expect(migrated.rightSidebar.collapsed).toBe(false);
  });

  it('Builtin presets are Layers left and Effect left', () => {
    const presets = builtinWorkspacePresets();
    expect(presets.map((p) => p.id)).toEqual(['builtin-layers-left', 'builtin-effect-left']);
    expect(presets[0]?.layout.left_order).toEqual(['layers']);
    expect(presets[0]?.layout.right_order).toEqual(['effect', 'colorlab']);
    expect(presets[1]?.layout.left_order).toEqual(['effect']);
    expect(presets[1]?.layout.right_order).toEqual(['layers', 'colorlab']);
  });
});

describe('9.1 cross-sidebar drag mode', () => {
  it('pointer over opposite rect → cross (not undock)', () => {
    expect(
      resolvePanelDragMode({
        clientX: 900,
        side: 'left',
        panelId: 'layers',
        ownRect: { left: 0, right: 300 },
        oppositeRect: { left: 800, right: 1100 },
        oppositeSide: 'right',
        viewportWidth: 1200,
      })
    ).toBe('cross');
  });

  it('pointer in canvas between sides → undock', () => {
    expect(
      resolvePanelDragMode({
        clientX: 500,
        side: 'left',
        panelId: 'layers',
        ownRect: { left: 0, right: 300 },
        oppositeRect: { left: 900, right: 1200 },
        oppositeSide: 'right',
        viewportWidth: 1200,
      })
    ).toBe('undock');
  });

  it('empty opposite uses edge strip', () => {
    expect(
      resolvePanelDragMode({
        clientX: 10,
        side: 'right',
        panelId: 'effect',
        ownRect: { left: 900, right: 1200 },
        oppositeRect: null,
        oppositeSide: 'left',
        viewportWidth: 1200,
      })
    ).toBe('cross');
  });

  it('inside own sidebar → reorder', () => {
    expect(
      resolvePanelDragMode({
        clientX: 100,
        side: 'left',
        panelId: 'layers',
        ownRect: { left: 0, right: 300 },
        oppositeRect: { left: 900, right: 1200 },
        oppositeSide: 'right',
        viewportWidth: 1200,
      })
    ).toBe('reorder');
  });
});

describe('9.2 per-side split ratios', () => {
  it('clamps ratio', () => {
    expect(clampSplitRatio(0.05)).toBe(0.2);
    expect(clampSplitRatio(0.95)).toBe(0.8);
    expect(clampSplitRatio(0.4)).toBe(0.4);
  });

  it('two-panel stack uses ratio weights', () => {
    expect(panelStackFlex(0, 2, 0.3)).toBe(0.3);
    expect(panelStackFlex(1, 2, 0.3)).toBe(0.7);
  });

  it('three+ panels share equally', () => {
    expect(panelStackFlex(0, 3, 0.2)).toBe(1);
    expect(panelStackFlex(2, 3, 0.2)).toBe(1);
  });
});

describe('9.3 workspace presets', () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => {
        store.set(key, value);
      },
      removeItem: (key: string) => {
        store.delete(key);
      },
      clear: () => store.clear(),
      key: (index: number) => Array.from(store.keys())[index] ?? null,
      get length() {
        return store.size;
      },
    } satisfies Storage);
  });

  it('lists builtins and can delete only user presets', () => {
    const list = listWorkspacePresets();
    expect(list.some((p) => p.id === 'builtin-layers-left')).toBe(true);
    expect(list.some((p) => p.id === 'builtin-effect-left')).toBe(true);
    expect(deleteWorkspacePreset('builtin-layers-left')).toBe(false);
  });

  it('snapshotFromPanelState keeps shell + orders', () => {
    const snap = snapshotFromPanelState(
      {
        panels: [
          {
            id: 'layers',
            docked: true,
            visible: true,
            window_label: null,
            saved_bounds: null,
            dock_side: 'left',
          },
        ],
        left_order: ['layers'],
        right_order: [],
      },
      {
        leftSidebar: { width: 300, collapsed: false },
        rightSidebar: { width: 332, collapsed: true },
        leftSplitRatio: 0.4,
        rightSplitRatio: 0.6,
      }
    );
    expect(snap.layout.left_order).toEqual(['layers']);
    expect(snap.shell.leftSplitRatio).toBe(0.4);
    expect(snap.shell.rightSidebar.collapsed).toBe(true);
  });
});
