import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { ReactNode } from 'react';
import {
  ShellProvider,
  useShell,
  getAutoExtractPalettesPref,
  migrateShellPrefs,
} from '../shell/ShellContext';

function wrapper({ children }: { children: ReactNode }) {
  return <ShellProvider>{children}</ShellProvider>;
}

function stubLocalStorage() {
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
  return store;
}

describe('ShellContext', () => {
  beforeEach(() => {
    stubLocalStorage();
  });

  it('exposes default dual sidebar prefs', () => {
    const { result } = renderHook(() => useShell(), { wrapper });
    expect(result.current.leftSidebar).toEqual({ width: 332, collapsed: false });
    expect(result.current.rightSidebar).toEqual({ width: 332, collapsed: false });
    expect(result.current.leftSplitRatio).toBe(0.5);
    expect(result.current.rightSplitRatio).toBe(0.5);
    expect(result.current.effectPanelRatio).toBe(0.5);
    expect(result.current.autoExtractPalettes).toBe(true);
  });

  it('updates split ratios independently per side', () => {
    const { result } = renderHook(() => useShell(), { wrapper });
    act(() => {
      result.current.setSplitRatio('left', 0.35);
      result.current.setSplitRatio('right', 0.65);
    });
    expect(result.current.leftSplitRatio).toBe(0.35);
    expect(result.current.rightSplitRatio).toBe(0.65);
    expect(result.current.effectPanelRatio).toBe(0.35);
  });

  it('updates width via functional setter per side', () => {
    const { result } = renderHook(() => useShell(), { wrapper });
    act(() => {
      result.current.setSidebarWidth('right', (w) => w + 20);
    });
    expect(result.current.rightSidebar.width).toBe(352);
    expect(result.current.leftSidebar.width).toBe(332);
  });

  it('collapses sides independently', () => {
    const { result } = renderHook(() => useShell(), { wrapper });
    act(() => {
      result.current.setSidebarCollapsed('left', true);
    });
    expect(result.current.leftSidebar.collapsed).toBe(true);
    expect(result.current.rightSidebar.collapsed).toBe(false);
  });

  it('persists dual prefs to localStorage', () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useShell(), { wrapper });
    act(() => {
      result.current.setSidebarCollapsed('left', true);
      result.current.setSidebarWidth('right', 400);
      result.current.setAutoExtractPalettes(false);
    });
    act(() => {
      vi.advanceTimersByTime(150);
    });
    const raw = localStorage.getItem('dither.shellPrefs');
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!);
    expect(parsed.version).toBe(2);
    expect(parsed.leftSidebar.collapsed).toBe(true);
    expect(parsed.rightSidebar.width).toBe(400);
    expect(parsed.autoExtractPalettes).toBe(false);
    expect(parsed.sidebarSide).toBeUndefined();
    vi.useRealTimers();
  });

  it('resetSidebarWidths restores default width', () => {
    const { result } = renderHook(() => useShell(), { wrapper });
    act(() => {
      result.current.setSidebarWidth('left', 500);
      result.current.setSidebarWidth('right', 280);
      result.current.resetSidebarWidths();
    });
    expect(result.current.leftSidebar.width).toBe(332);
    expect(result.current.rightSidebar.width).toBe(332);
  });

  it('getAutoExtractPalettesPref defaults to true and reads storage', () => {
    expect(getAutoExtractPalettesPref()).toBe(true);
    localStorage.setItem(
      'dither.shellPrefs',
      JSON.stringify({ version: 2, autoExtractPalettes: false, leftSidebar: { width: 332, collapsed: false }, rightSidebar: { width: 332, collapsed: false }, effectPanelRatio: 0.5 })
    );
    expect(getAutoExtractPalettesPref()).toBe(false);
  });

  it('migrates v1 exclusive sidebarSide=left', () => {
    const migrated = migrateShellPrefs({
      sidebarSide: 'left',
      sidebarWidth: 400,
      sidebarCollapsed: true,
      effectPanelRatio: 0.4,
      autoExtractPalettes: true,
    });
    expect(migrated.version).toBe(2);
    expect(migrated.leftSidebar).toEqual({ width: 400, collapsed: true });
    expect(migrated.rightSidebar).toEqual({ width: 332, collapsed: false });
    expect(migrated.leftSplitRatio).toBe(0.4);
    expect(migrated.rightSplitRatio).toBe(0.4);
    expect(migrated.effectPanelRatio).toBe(0.4);
  });

  it('migrates v1 exclusive sidebarSide=right by default', () => {
    const migrated = migrateShellPrefs({
      sidebarWidth: 280,
      sidebarCollapsed: false,
    });
    expect(migrated.rightSidebar).toEqual({ width: 280, collapsed: false });
    expect(migrated.leftSidebar).toEqual({ width: 332, collapsed: false });
  });

  it('loads migrated v1 prefs from localStorage on mount', () => {
    localStorage.setItem(
      'dither.shellPrefs',
      JSON.stringify({
        sidebarSide: 'left',
        sidebarWidth: 360,
        sidebarCollapsed: true,
        effectPanelRatio: 0.5,
        autoExtractPalettes: true,
      })
    );
    const { result } = renderHook(() => useShell(), { wrapper });
    expect(result.current.leftSidebar).toEqual({ width: 360, collapsed: true });
    expect(result.current.rightSidebar).toEqual({ width: 332, collapsed: false });
    const stored = JSON.parse(localStorage.getItem('dither.shellPrefs')!);
    expect(stored.version).toBe(2);
    expect(stored.sidebarSide).toBeUndefined();
  });
});
