import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  clampSplitRatio,
  panelStackFlex,
  resolvePanelDragMode,
  sidebarColumnWidth,
} from '../../features/panels/panelDragMode';
import {
  applyFlexPanelLayout,
  builtinWorkspacePresets,
  deleteWorkspacePreset,
  listWorkspacePresets,
  snapshotFromFlexSides,
} from '../../features/panels/workspacePresets';
import { migrateShellPrefs } from '../shell/ShellContext';

describe('FlexLayout dual-sidebar shell width', () => {
  it('Default: Layers left, Effect+Color Lab right → both columns have width', () => {
    expect(sidebarColumnWidth(true, false, 332)).toBe(332);
    expect(sidebarColumnWidth(true, false, 332)).toBe(332);
  });

  it('Empty side (no docked flex) → width 0', () => {
    expect(sidebarColumnWidth(false, false, 332)).toBe(0);
    expect(sidebarColumnWidth(false, true, 332)).toBe(0);
  });

  it('Collapsed docked side → 40px strip', () => {
    expect(sidebarColumnWidth(true, true, 332)).toBe(40);
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

describe('9.3 workspace presets (Flex)', () => {
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

  it('snapshotFromFlexSides keeps shell + orders', () => {
    const snap = snapshotFromFlexSides(
      ['layers'],
      ['effect', 'colorlab'],
      {
        leftSidebar: { width: 300, collapsed: false },
        rightSidebar: { width: 332, collapsed: true },
        leftSplitRatio: 0.4,
        rightSplitRatio: 0.6,
      }
    );
    expect(snap.layout.left_order).toEqual(['layers']);
    expect(snap.layout.right_order).toEqual(['effect', 'colorlab']);
    expect(snap.shell.leftSplitRatio).toBe(0.4);
    expect(snap.shell.rightSidebar.collapsed).toBe(true);
  });

  it('applyFlexPanelLayout moves panels via handlers', () => {
    const moves: Array<[string, string]> = [];
    applyFlexPanelLayout(
      { left_order: ['effect'], right_order: ['layers', 'colorlab'] },
      {
        movePanelBetweenSides: (id, to) => {
          moves.push([id, to]);
        },
      }
    );
    expect(moves).toEqual([
      ['effect', 'left'],
      ['layers', 'right'],
      ['colorlab', 'right'],
    ]);
  });
});
