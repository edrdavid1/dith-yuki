import type { DockSide, PanelId } from '../../types/panels';
import type { SidebarGeom } from '../../app/shell/ShellContext';

export type WorkspaceShellSnapshot = {
  leftSidebar: SidebarGeom;
  rightSidebar: SidebarGeom;
  leftSplitRatio: number;
  rightSplitRatio: number;
};

/** Flex-owned panel placement (left/right sidebars only; Preview stays center). */
export type WorkspaceLayoutSnapshot = {
  left_order: PanelId[];
  right_order: PanelId[];
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

/** Built-in named layouts (not user-deletable). */
export function builtinWorkspacePresets(): WorkspacePreset[] {
  return [
    {
      id: 'builtin-layers-left',
      name: 'Layers left',
      builtin: true,
      shell: { ...DEFAULT_SHELL },
      layout: {
        left_order: ['layers'],
        right_order: ['effect', 'colorlab'],
      },
    },
    {
      id: 'builtin-effect-left',
      name: 'Effect left',
      builtin: true,
      shell: { ...DEFAULT_SHELL },
      layout: {
        left_order: ['effect'],
        right_order: ['layers', 'colorlab'],
      },
    },
  ];
}

function isUserPreset(value: unknown): value is WorkspacePreset {
  if (!value || typeof value !== 'object') return false;
  const p = value as Record<string, unknown>;
  return (
    typeof p.id === 'string' &&
    typeof p.name === 'string' &&
    p.builtin !== true &&
    p.layout != null &&
    typeof p.layout === 'object' &&
    p.shell != null &&
    typeof p.shell === 'object'
  );
}

function normalizeLayout(raw: unknown): WorkspaceLayoutSnapshot {
  const layout = (raw ?? {}) as Record<string, unknown>;
  const left = Array.isArray(layout.left_order)
    ? (layout.left_order as string[]).filter((id): id is PanelId =>
        DOCKABLE.includes(id as PanelId)
      )
    : [];
  const right = Array.isArray(layout.right_order)
    ? (layout.right_order as string[]).filter((id): id is PanelId =>
        DOCKABLE.includes(id as PanelId)
      )
    : [];
  return { left_order: left, right_order: right };
}

function readUserPresets(): WorkspacePreset[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isUserPreset).map((p) => ({
      ...p,
      layout: normalizeLayout(p.layout),
      shell: {
        leftSidebar: { ...(p.shell as WorkspaceShellSnapshot).leftSidebar },
        rightSidebar: { ...(p.shell as WorkspaceShellSnapshot).rightSidebar },
        leftSplitRatio: (p.shell as WorkspaceShellSnapshot).leftSplitRatio ?? 0.5,
        rightSplitRatio: (p.shell as WorkspaceShellSnapshot).rightSplitRatio ?? 0.5,
      },
    }));
  } catch {
    return [];
  }
}

function writeUserPresets(presets: WorkspacePreset[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(presets));
  } catch {
    /* ignore quota */
  }
}

export function listWorkspacePresets(): WorkspacePreset[] {
  return [...builtinWorkspacePresets(), ...readUserPresets()];
}

export function snapshotFromFlexSides(
  leftComponents: string[],
  rightComponents: string[],
  shell: WorkspaceShellSnapshot
): { layout: WorkspaceLayoutSnapshot; shell: WorkspaceShellSnapshot } {
  const left_order = leftComponents.filter((id): id is PanelId =>
    DOCKABLE.includes(id as PanelId)
  );
  const right_order = rightComponents.filter((id): id is PanelId =>
    DOCKABLE.includes(id as PanelId)
  );
  return {
    shell: {
      leftSidebar: { ...shell.leftSidebar },
      rightSidebar: { ...shell.rightSidebar },
      leftSplitRatio: shell.leftSplitRatio,
      rightSplitRatio: shell.rightSplitRatio,
    },
    layout: { left_order, right_order },
  };
}

export function captureWorkspacePreset(
  name: string,
  shell: WorkspaceShellSnapshot,
  leftComponents: string[],
  rightComponents: string[]
): WorkspacePreset {
  const snap = snapshotFromFlexSides(leftComponents, rightComponents, shell);
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

export type ApplyFlexHandlers = {
  /** Move a dockable panel onto `to` (no-op if already there). */
  movePanelBetweenSides: (panelComponent: string, to: 'left' | 'right') => void;
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
 * Drive FlexLayout side placement to match a layout snapshot.
 * Order: place left targets first, then right (movePanelBetweenSides is idempotent).
 */
export function applyFlexPanelLayout(
  target: WorkspaceLayoutSnapshot,
  handlers: ApplyFlexHandlers
): void {
  for (const id of target.left_order) {
    handlers.movePanelBetweenSides(id, 'left');
  }
  for (const id of target.right_order) {
    handlers.movePanelBetweenSides(id, 'right');
  }
}

export function applyWorkspacePreset(
  preset: WorkspacePreset,
  shellHandlers: ApplyShellHandlers,
  flexHandlers: ApplyFlexHandlers
): void {
  applyWorkspaceShell(preset.shell, shellHandlers);
  applyFlexPanelLayout(preset.layout, flexHandlers);
}
