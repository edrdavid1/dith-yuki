import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import type { DockSide } from '../../types/panels';
import { clampSplitRatio } from '../../features/panels/panelDragMode';
import {
  DEFAULT_PREVIEW_BACKGROUND,
  parsePreviewBackground,
  type PreviewBackground,
} from '../../features/preview/previewBackground';

export type SidebarGeom = {
  width: number;
  collapsed: boolean;
};

export type ShellState = {
  leftSidebar: SidebarGeom;
  rightSidebar: SidebarGeom;
  /** Vertical split weight for left stack (first panel share when 2 panels). */
  leftSplitRatio: number;
  /** Vertical split weight for right stack. */
  rightSplitRatio: number;
  /**
   * @deprecated Prefer leftSplitRatio / rightSplitRatio. Kept as alias of leftSplitRatio.
   */
  effectPanelRatio: number;
  /** When true, extract a palette after open/import of an image (default on). */
  autoExtractPalettes: boolean;
  /** Canvas fill behind the preview image: gray, black, or pattern. */
  previewBackground: PreviewBackground;
  setSidebarWidth: (side: DockSide, width: number | ((prev: number) => number)) => void;
  setSidebarCollapsed: (side: DockSide, collapsed: boolean) => void;
  resetSidebarWidths: () => void;
  setSplitRatio: (side: DockSide, ratio: number | ((prev: number) => number)) => void;
  /** @deprecated Sets leftSplitRatio. */
  setEffectPanelRatio: (ratio: number | ((prev: number) => number)) => void;
  /** Exchange left/right sidebar widths, collapse, and split ratios. */
  swapSidebars: () => void;
  setAutoExtractPalettes: (enabled: boolean) => void;
  setPreviewBackground: (kind: PreviewBackground) => void;
};

/** v2 persisted shape (additive split ratios). */
export type PersistedShellPrefsV2 = {
  version: 2;
  leftSidebar: SidebarGeom;
  rightSidebar: SidebarGeom;
  leftSplitRatio: number;
  rightSplitRatio: number;
  /** Legacy single ratio; still written for older floating windows. */
  effectPanelRatio: number;
  autoExtractPalettes: boolean;
  previewBackground: PreviewBackground;
};

/** Legacy v1 keys (exclusive single sidebar). */
type PersistedShellPrefsV1 = {
  sidebarSide?: DockSide;
  sidebarCollapsed?: boolean;
  sidebarWidth?: number;
  effectPanelRatio?: number;
  autoExtractPalettes?: boolean;
  previewBackground?: PreviewBackground | string;
};

const DEFAULT_AUTO_EXTRACT_PALETTES = true;
const DEFAULT_SIDEBAR_WIDTH = 332;
const DEFAULT_SPLIT_RATIO = 0.5;

const DEFAULT_SIDEBAR_GEOM: SidebarGeom = {
  width: DEFAULT_SIDEBAR_WIDTH,
  collapsed: false,
};

const ShellContext = createContext<ShellState | null>(null);

const SHELL_PREFS_KEY = 'dither.shellPrefs';
const SHELL_PREFS_CHANNEL = 'dither.shellPrefs';
/** Debounce persist/broadcast so sidebar drag isn't fighting localStorage + BC every pixel. */
const SHELL_PERSIST_DEBOUNCE_MS = 120;

type ShellBroadcastMessage = PersistedShellPrefsV2 & { origin: string };

function createShellInstanceId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID();
    }
  } catch {
    /* ignore */
  }
  return `shell-${Math.random().toString(36).slice(2)}`;
}

function isDockSide(value: unknown): value is DockSide {
  return value === 'left' || value === 'right';
}

function isSidebarGeom(value: unknown): value is SidebarGeom {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as SidebarGeom).width === 'number' &&
    typeof (value as SidebarGeom).collapsed === 'boolean'
  );
}

function getLocalStorage(): Storage | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null;
  } catch {
    return null;
  }
}

function defaultPrefs(): PersistedShellPrefsV2 {
  return {
    version: 2,
    leftSidebar: { ...DEFAULT_SIDEBAR_GEOM },
    rightSidebar: { ...DEFAULT_SIDEBAR_GEOM },
    leftSplitRatio: DEFAULT_SPLIT_RATIO,
    rightSplitRatio: DEFAULT_SPLIT_RATIO,
    effectPanelRatio: DEFAULT_SPLIT_RATIO,
    autoExtractPalettes: DEFAULT_AUTO_EXTRACT_PALETTES,
    previewBackground: DEFAULT_PREVIEW_BACKGROUND,
  };
}

function readSplitRatios(obj: Record<string, unknown>): {
  leftSplitRatio: number;
  rightSplitRatio: number;
} {
  const legacy =
    typeof obj.effectPanelRatio === 'number' ? obj.effectPanelRatio : DEFAULT_SPLIT_RATIO;
  return {
    leftSplitRatio: clampSplitRatio(
      typeof obj.leftSplitRatio === 'number' ? obj.leftSplitRatio : legacy
    ),
    rightSplitRatio: clampSplitRatio(
      typeof obj.rightSplitRatio === 'number' ? obj.rightSplitRatio : legacy
    ),
  };
}

/**
 * Migrate legacy exclusive-sidebar prefs (or already-v2) into dual shape.
 * Exported for unit tests.
 */
export function migrateShellPrefs(raw: unknown): PersistedShellPrefsV2 {
  const base = defaultPrefs();
  if (!raw || typeof raw !== 'object') {
    return base;
  }
  const obj = raw as Record<string, unknown>;

  if (obj.version === 2 || isSidebarGeom(obj.leftSidebar) || isSidebarGeom(obj.rightSidebar)) {
    const splits = readSplitRatios(obj);
    return {
      version: 2,
      leftSidebar: isSidebarGeom(obj.leftSidebar)
        ? { ...obj.leftSidebar }
        : { ...DEFAULT_SIDEBAR_GEOM },
      rightSidebar: isSidebarGeom(obj.rightSidebar)
        ? { ...obj.rightSidebar }
        : { ...DEFAULT_SIDEBAR_GEOM },
      leftSplitRatio: splits.leftSplitRatio,
      rightSplitRatio: splits.rightSplitRatio,
      effectPanelRatio: splits.leftSplitRatio,
      autoExtractPalettes:
        typeof obj.autoExtractPalettes === 'boolean'
          ? obj.autoExtractPalettes
          : DEFAULT_AUTO_EXTRACT_PALETTES,
      previewBackground: parsePreviewBackground(obj.previewBackground),
    };
  }

  const v1 = obj as PersistedShellPrefsV1;
  const side: DockSide = isDockSide(v1.sidebarSide) ? v1.sidebarSide : 'right';
  const other: DockSide = side === 'left' ? 'right' : 'left';
  const migrated: SidebarGeom = {
    width: typeof v1.sidebarWidth === 'number' ? v1.sidebarWidth : DEFAULT_SIDEBAR_WIDTH,
    collapsed: typeof v1.sidebarCollapsed === 'boolean' ? v1.sidebarCollapsed : false,
  };

  const prefs = defaultPrefs();
  prefs[side === 'left' ? 'leftSidebar' : 'rightSidebar'] = migrated;
  prefs[other === 'left' ? 'leftSidebar' : 'rightSidebar'] = { ...DEFAULT_SIDEBAR_GEOM };
  if (typeof v1.effectPanelRatio === 'number') {
    const r = clampSplitRatio(v1.effectPanelRatio);
    prefs.leftSplitRatio = r;
    prefs.rightSplitRatio = r;
    prefs.effectPanelRatio = r;
  }
  if (typeof v1.autoExtractPalettes === 'boolean') {
    prefs.autoExtractPalettes = v1.autoExtractPalettes;
  }
  prefs.previewBackground = parsePreviewBackground(v1.previewBackground);
  return prefs;
}

function readPersistedPrefs(): PersistedShellPrefsV2 {
  try {
    const store = getLocalStorage();
    const raw = store?.getItem(SHELL_PREFS_KEY);
    if (!raw) return defaultPrefs();
    const parsed = JSON.parse(raw);
    const migrated = migrateShellPrefs(parsed);
    const wasV1 =
      !parsed ||
      typeof parsed !== 'object' ||
      ((parsed as { version?: number }).version !== 2 &&
        !isSidebarGeom((parsed as { leftSidebar?: unknown }).leftSidebar));
    if (wasV1) {
      writePersistedPrefs(migrated);
    }
    return migrated;
  } catch {
    return defaultPrefs();
  }
}

/** Read auto-extract preference outside React (thunks / listeners). Default: true. */
export function getAutoExtractPalettesPref(): boolean {
  return readPersistedPrefs().autoExtractPalettes;
}

function writePersistedPrefs(prefs: PersistedShellPrefsV2) {
  try {
    getLocalStorage()?.setItem(SHELL_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // ignore quota / private mode
  }
}

function applyPrefsPatch(
  parsed: PersistedShellPrefsV2,
  setters: {
    setLeftSidebar: (g: SidebarGeom) => void;
    setRightSidebar: (g: SidebarGeom) => void;
    setLeftSplitRatio: (ratio: number) => void;
    setRightSplitRatio: (ratio: number) => void;
    setAutoExtractPalettes: (enabled: boolean) => void;
    setPreviewBackground: (kind: PreviewBackground) => void;
  }
) {
  setters.setLeftSidebar({ ...parsed.leftSidebar });
  setters.setRightSidebar({ ...parsed.rightSidebar });
  setters.setLeftSplitRatio(parsed.leftSplitRatio);
  setters.setRightSplitRatio(parsed.rightSplitRatio);
  setters.setAutoExtractPalettes(parsed.autoExtractPalettes);
  setters.setPreviewBackground(parsed.previewBackground);
}

/**
 * App shell layout prefs — persisted and synced across main + floating windows.
 * Document / selection / panels stay in RTK.
 */
export function ShellProvider({ children }: { children: ReactNode }) {
  const initial = readPersistedPrefs();
  const instanceIdRef = useRef(createShellInstanceId());

  const [leftSidebar, setLeftSidebarState] = useState<SidebarGeom>(initial.leftSidebar);
  const [rightSidebar, setRightSidebarState] = useState<SidebarGeom>(initial.rightSidebar);
  const [leftSplitRatio, setLeftSplitRatioState] = useState(initial.leftSplitRatio);
  const [rightSplitRatio, setRightSplitRatioState] = useState(initial.rightSplitRatio);
  const [autoExtractPalettes, setAutoExtractPalettesState] = useState(
    initial.autoExtractPalettes
  );
  const [previewBackground, setPreviewBackgroundState] = useState(initial.previewBackground);

  const prefsRef = useRef<PersistedShellPrefsV2>({
    version: 2,
    leftSidebar,
    rightSidebar,
    leftSplitRatio,
    rightSplitRatio,
    effectPanelRatio: leftSplitRatio,
    autoExtractPalettes,
    previewBackground,
  });
  prefsRef.current = {
    version: 2,
    leftSidebar,
    rightSidebar,
    leftSplitRatio,
    rightSplitRatio,
    effectPanelRatio: leftSplitRatio,
    autoExtractPalettes,
    previewBackground,
  };

  const persistPrefs = useCallback((prefs: PersistedShellPrefsV2) => {
    writePersistedPrefs(prefs);
    try {
      const channel = new BroadcastChannel(SHELL_PREFS_CHANNEL);
      const message: ShellBroadcastMessage = {
        ...prefs,
        origin: instanceIdRef.current,
      };
      channel.postMessage(message);
      channel.close();
    } catch {
      // BroadcastChannel unavailable
    }
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      persistPrefs(prefsRef.current);
    }, SHELL_PERSIST_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [leftSidebar, rightSidebar, leftSplitRatio, rightSplitRatio, autoExtractPalettes, previewBackground, persistPrefs]);

  // Flush latest prefs if the provider unmounts mid-debounce.
  useEffect(() => {
    return () => {
      persistPrefs(prefsRef.current);
    };
  }, [persistPrefs]);

  useEffect(() => {
    const onRemote = (data: unknown) => {
      const parsed = migrateShellPrefs(data);
      applyPrefsPatch(parsed, {
        setLeftSidebar: setLeftSidebarState,
        setRightSidebar: setRightSidebarState,
        setLeftSplitRatio: setLeftSplitRatioState,
        setRightSplitRatio: setRightSplitRatioState,
        setAutoExtractPalettes: setAutoExtractPalettesState,
        setPreviewBackground: setPreviewBackgroundState,
      });
    };

    let channel: BroadcastChannel | null = null;
    try {
      channel = new BroadcastChannel(SHELL_PREFS_CHANNEL);
      channel.onmessage = (event) => {
        const data = event.data;
        if (!data || typeof data !== 'object') return;
        // Same-window BC echo was re-applying prefs mid-drag and fighting the pointer.
        if ((data as { origin?: unknown }).origin === instanceIdRef.current) return;
        onRemote(data);
      };
    } catch {
      channel = null;
    }

    const onStorage = (e: StorageEvent) => {
      if (e.key !== SHELL_PREFS_KEY || e.newValue == null) return;
      try {
        onRemote(JSON.parse(e.newValue));
      } catch {
        // ignore malformed
      }
    };
    window.addEventListener('storage', onStorage);

    return () => {
      window.removeEventListener('storage', onStorage);
      channel?.close();
    };
  }, []);

  const setSidebarWidth = useCallback(
    (side: DockSide, width: number | ((prev: number) => number)) => {
      const apply = (prev: SidebarGeom): SidebarGeom => ({
        ...prev,
        width: typeof width === 'function' ? width(prev.width) : width,
      });
      if (side === 'left') {
        setLeftSidebarState(apply);
      } else {
        setRightSidebarState(apply);
      }
    },
    []
  );

  const setSidebarCollapsed = useCallback((side: DockSide, collapsed: boolean) => {
    if (side === 'left') {
      setLeftSidebarState((prev) => ({ ...prev, collapsed }));
    } else {
      setRightSidebarState((prev) => ({ ...prev, collapsed }));
    }
  }, []);

  const resetSidebarWidths = useCallback(() => {
    setLeftSidebarState((prev) => ({ ...prev, width: DEFAULT_SIDEBAR_WIDTH }));
    setRightSidebarState((prev) => ({ ...prev, width: DEFAULT_SIDEBAR_WIDTH }));
  }, []);

  const setSplitRatio = useCallback(
    (side: DockSide, ratio: number | ((prev: number) => number)) => {
      const apply = (prev: number) =>
        clampSplitRatio(typeof ratio === 'function' ? ratio(prev) : ratio);
      if (side === 'left') {
        setLeftSplitRatioState(apply);
      } else {
        setRightSplitRatioState(apply);
      }
    },
    []
  );

  const setEffectPanelRatio = useCallback((ratio: number | ((prev: number) => number)) => {
    setSplitRatio('left', ratio);
  }, [setSplitRatio]);

  const setAutoExtractPalettes = useCallback((enabled: boolean) => {
    setAutoExtractPalettesState(enabled);
  }, []);

  const setPreviewBackground = useCallback((kind: PreviewBackground) => {
    setPreviewBackgroundState(parsePreviewBackground(kind));
  }, []);

  const swapSidebars = useCallback(() => {
    const prefs = prefsRef.current;
    setLeftSidebarState({ ...prefs.rightSidebar });
    setRightSidebarState({ ...prefs.leftSidebar });
    setLeftSplitRatioState(prefs.rightSplitRatio);
    setRightSplitRatioState(prefs.leftSplitRatio);
  }, []);

  const value = useMemo<ShellState>(
    () => ({
      leftSidebar,
      rightSidebar,
      leftSplitRatio,
      rightSplitRatio,
      effectPanelRatio: leftSplitRatio,
      autoExtractPalettes,
      previewBackground,
      setSidebarWidth,
      setSidebarCollapsed,
      resetSidebarWidths,
      setSplitRatio,
      setEffectPanelRatio,
      swapSidebars,
      setAutoExtractPalettes,
      setPreviewBackground,
    }),
    [
      leftSidebar,
      rightSidebar,
      leftSplitRatio,
      rightSplitRatio,
      autoExtractPalettes,
      previewBackground,
      setSidebarWidth,
      setSidebarCollapsed,
      resetSidebarWidths,
      setSplitRatio,
      setEffectPanelRatio,
      swapSidebars,
      setAutoExtractPalettes,
      setPreviewBackground,
    ]
  );

  return <ShellContext.Provider value={value}>{children}</ShellContext.Provider>;
}

export function useShell(): ShellState {
  const ctx = useContext(ShellContext);
  if (!ctx) {
    throw new Error('useShell must be used within ShellProvider');
  }
  return ctx;
}
