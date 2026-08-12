import type { DockSide, PanelId, PanelInfo, PanelStateSnapshot } from '../../types/panels';
import { FLOATING_ONLY_PANELS } from '../../types/panels';
import type { SidebarGeom } from '../../app/shell/ShellContext';
import {
  dockPanelAt,
  getPanelsState,
  hidePanel,
  movePanelToSide,
  reorderSidebar,
  showPanel,
  undockPanel,
} from '../../shared/ipc/panels';

export type WorkspaceShellSnapshot = {
  leftSidebar: SidebarGeom;
  rightSidebar: SidebarGeom;
  leftSplitRatio: number;
  rightSplitRatio: number;
};

export type WorkspaceLayoutSnapshot = {
  left_order: PanelId[];
  right_order: PanelId[];
  panels: Array<Pick<PanelInfo, 'id' | 'docked' | 'visible' | 'dock_side'>>;
};

export type WorkspacePreset = {
  id: string;
  name: string;
  builtin?: boolean;
  layout: WorkspaceLayoutSnapshot;
  shell: WorkspaceShellSnapshot;
};

const STORAGE_KEY = 'dither.workspacePresets';
const DOCKABLE: PanelId[] = ['layers', 'effect', 'colorlab'];

const DEFAULT_SHELL: WorkspaceShellSnapshot = {
  leftSidebar: { width: 332, collapsed: false },
  rightSidebar: { width: 332, collapsed: false },
  leftSplitRatio: 0.5,
  rightSplitRatio: 0.5,
};

function dockablePanel(
  id: PanelId,
  dock_side: DockSide,
  visible = true
): WorkspaceLayoutSnapshot['panels'][number] {
  return { id, docked: true, visible, dock_side };
}

/** Built-in named layouts (not user-deletable). */
export function builtinWorkspacePresets(): WorkspacePreset[] {
  return [
    {
      id: 'builtin-default',
      name: 'Default (Layers left)',
      builtin: true,
      shell: { ...DEFAULT_SHELL },
      layout: {
        left_order: ['layers'],
        right_order: ['effect', 'colorlab'],
        panels: [
          dockablePanel('layers', 'left'),
          dockablePanel('effect', 'right'),
          dockablePanel('colorlab', 'right'),
        ],
      },
    },
    {
      id: 'builtin-all-left',
      name: 'All panels left',
      builtin: true,
      shell: { ...DEFAULT_SHELL },
      layout: {
        left_order: ['layers', 'effect', 'colorlab'],
        right_order: [],
        panels: [
          dockablePanel('layers', 'left'),
          dockablePanel('effect', 'left'),
          dockablePanel('colorlab', 'left'),
        ],
      },
    },
    {
      id: 'builtin-all-right',
      name: 'All panels right',
      builtin: true,
      shell: { ...DEFAULT_SHELL },
      layout: {
        left_order: [],
        right_order: ['layers', 'effect', 'colorlab'],
        panels: [
          dockablePanel('layers', 'right'),
          dockablePanel('effect', 'right'),
          dockablePanel('colorlab', 'right'),
        ],
      },
    },
  ];
}

function readUserPresets(): WorkspacePreset[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isUserPreset);
  } catch {
    return [];
  }
}

function isUserPreset(value: unknown): value is WorkspacePreset {
  if (!value || typeof value !== 'object') return false;
  const p = value as WorkspacePreset;
  return (
    typeof p.id === 'string' &&
    typeof p.name === 'string' &&
    !p.builtin &&
    Array.isArray(p.layout?.left_order) &&
    Array.isArray(p.layout?.right_order) &&
    Array.isArray(p.layout?.panels)
  );
}

function writeUserPresets(presets: WorkspacePreset[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(presets));
  } catch {
    // ignore
  }
}

export function listWorkspacePresets(): WorkspacePreset[] {
  return [...builtinWorkspacePresets(), ...readUserPresets()];
}

export function snapshotFromPanelState(
  state: PanelStateSnapshot,
  shell: WorkspaceShellSnapshot
): { layout: WorkspaceLayoutSnapshot; shell: WorkspaceShellSnapshot } {
  return {
    shell: {
      leftSidebar: { ...shell.leftSidebar },
      rightSidebar: { ...shell.rightSidebar },
      leftSplitRatio: shell.leftSplitRatio,
      rightSplitRatio: shell.rightSplitRatio,
    },
    layout: {
      left_order: [...state.left_order],
      right_order: [...state.right_order],
      panels: state.panels
        .filter((p) => !FLOATING_ONLY_PANELS.has(p.id as PanelId))
        .map((p) => ({
          id: p.id as PanelId,
          docked: p.docked,
          visible: p.visible,
          dock_side: p.dock_side,
        })),
    },
  };
}

export async function captureWorkspacePreset(
  name: string,
  shell: WorkspaceShellSnapshot
): Promise<WorkspacePreset> {
  const state = await getPanelsState();
  const snap = snapshotFromPanelState(state, shell);
  const preset: WorkspacePreset = {
    id: `user-${Date.now()}`,
    name: name.trim() || 'Custom layout',
    layout: snap.layout,
    shell: snap.shell,
  };
  const next = [...readUserPresets(), preset];
  writeUserPresets(next);
  return preset;
}

export function deleteWorkspacePreset(id: string): boolean {
  const users = readUserPresets();
  const next = users.filter((p) => p.id !== id);
  if (next.length === users.length) return false;
  writeUserPresets(next);
  return true;
}

export type ApplyShellHandlers = {
  setSidebarWidth: (side: DockSide, width: number) => void;
  setSidebarCollapsed: (side: DockSide, collapsed: boolean) => void;
  setSplitRatio: (side: DockSide, ratio: number) => void;
};

export function applyWorkspaceShell(
  shell: WorkspaceShellSnapshot,
  handlers: ApplyShellHandlers
): void {
  handlers.setSidebarWidth('left', shell.leftSidebar.width);
  handlers.setSidebarWidth('right', shell.rightSidebar.width);
  handlers.setSidebarCollapsed('left', shell.leftSidebar.collapsed);
  handlers.setSidebarCollapsed('right', shell.rightSidebar.collapsed);
  handlers.setSplitRatio('left', shell.leftSplitRatio);
  handlers.setSplitRatio('right', shell.rightSplitRatio);
}

/**
 * Drive existing panel IPC to match a layout snapshot (no float windows for
 * panels that remain docked).
 */
export async function applyPanelLayout(target: WorkspaceLayoutSnapshot): Promise<void> {
  const wantById = new Map(target.panels.map((p) => [p.id, p]));
  let current = await getPanelsState();
  const curById = () => new Map(current.panels.map((p) => [p.id, p]));

  for (const id of DOCKABLE) {
    const want = wantById.get(id);
    if (!want) continue;
    const now = curById().get(id);
    if (!now) continue;

    if (want.docked && want.dock_side) {
      if (!now.docked) {
        await dockPanelAt(id, want.dock_side, Number.MAX_SAFE_INTEGER);
      } else if (now.dock_side !== want.dock_side) {
        await movePanelToSide(id, want.dock_side);
      }
    } else if (!want.docked && now.docked) {
      await undockPanel(id);
    }
    current = await getPanelsState();
  }

  // Place members onto sides in target order (append then reorder).
  for (const [side, order] of [
    ['left', target.left_order],
    ['right', target.right_order],
  ] as const) {
    for (const id of order) {
      const now = curById().get(id);
      if (!now?.docked) continue;
      if (now.dock_side !== side) {
        await movePanelToSide(id, side);
        current = await getPanelsState();
      }
    }
  }

  if (target.left_order.length > 0) {
    await reorderSidebar('left', [...target.left_order]);
  }
  if (target.right_order.length > 0) {
    await reorderSidebar('right', [...target.right_order]);
  }

  current = await getPanelsState();
  for (const id of DOCKABLE) {
    const want = wantById.get(id);
    const now = curById().get(id);
    if (!want || !now) continue;
    if (want.visible && !now.visible) await showPanel(id);
    if (!want.visible && now.visible) await hidePanel(id);
  }
}

export async function applyWorkspacePreset(
  preset: WorkspacePreset,
  handlers: ApplyShellHandlers
): Promise<void> {
  applyWorkspaceShell(preset.shell, handlers);
  await applyPanelLayout(preset.layout);
}
