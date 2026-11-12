/**
 * LayoutContext — React bindings for the LayoutStore singleton (W4).
 *
 * Models are owned by `layoutStore` (module-level, HMR-preserved).
 * This provider only subscribes via useSyncExternalStore and exposes the
 * public useLayoutContext() API unchanged.
 */

import React, {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useSyncExternalStore,
} from 'react';
import type { Model } from 'flexlayout-react';
import type { FlexSide } from '../defaults/DefaultLayouts';
export {
  maxDockedHorizontalSpan,
  sidebarWidthForHorizontalSpan,
} from '../features/panels/horizontalDock';
import { layoutStore } from './layoutStore';
import { findTabByComponent } from './layoutModelOps';
import { runFlexPopoutInvariantCheck } from '../features/panels/flexPopoutInvariants';

// Re-export model helpers for existing imports / tests.
export {
  applyAppChromePolicy,
  dockAllFloating,
  emptySideJson,
  findFirstTabSetId,
  findTabByComponent,
  listDockedFlexComponents,
  listFlexComponents,
  listFloatingFlexComponents,
  modelForSide,
  normalizeSideToVerticalStack,
  rebalanceFloatingTabsets,
  tabDisplayName,
} from './layoutModelOps';

// ─── Context type ─────────────────────────────────────────────────────────────

export interface SideModelState {
  model: Model | null;
  setModel: (m: Model) => void;
  isLoading: boolean;
}

export interface LayoutContextType {
  left: SideModelState;
  right: SideModelState;
  /** Preview canvas host (not a sidebar). */
  center: SideModelState;
  /**
   * Increments when a side model is mutated in place (float/dock). Consumers that
   * derive docked vs floating must read this so React re-renders — Model identity
   * stays the same on purpose (cloning kills FlexLayout popouts).
   */
  layoutEpoch: number;
  /** One-shot layout migration / recovery message for the shell toast. */
  layoutToast: string | null;
  clearLayoutToast: () => void;
  /** Move a FlexLayout tab (by component id) from one sidebar model to the other. */
  movePanelBetweenSides: (panelComponent: string, to: 'left' | 'right') => void;
  /** Swap left/right FlexLayout models (titlebar sidebar-swap button). */
  swapFlexSides: () => void;
  /** Pop a docked tab into a FlexLayout OS popout window. */
  floatPanel: (side: FlexSide, panelComponent: string) => void;
  /** Dock a floated tab back into its host (sidebar or center canvas). */
  dockPanel: (side: FlexSide, panelComponent: string) => void;
  /**
   * Redock a floated FlexLayout panel onto `to` (same side = unfloat,
   * opposite = move + dock). Preview always returns to center.
   */
  redockFlexPanelToSide: (panelComponent: string, to: 'left' | 'right') => void;
  /**
   * W2: dock every floating tab on left/center/right (and close their OS shells).
   * Used by Window → "Return all panels to dock" and recovery paths.
   */
  returnAllPanelsToDock: () => void;
}

const LayoutContext = createContext<LayoutContextType | undefined>(undefined);

// ─── Provider ────────────────────────────────────────────────────────────────

export const LayoutProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const snapshot = useSyncExternalStore(
    layoutStore.subscribe,
    layoutStore.getSnapshot,
    layoutStore.getSnapshot,
  );

  useEffect(() => layoutStore.ensureInitialized(), []);

  // W6: DEV registry ↔ window ↔ floating-tab invariants.
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    if (snapshot.leftLoading || snapshot.rightLoading || snapshot.centerLoading) {
      return;
    }
    const t = window.setTimeout(() => {
      void runFlexPopoutInvariantCheck({
        left: snapshot.left,
        center: snapshot.center,
        right: snapshot.right,
      });
    }, 250);
    return () => clearTimeout(t);
  }, [
    snapshot.version,
    snapshot.left,
    snapshot.center,
    snapshot.right,
    snapshot.leftLoading,
    snapshot.rightLoading,
    snapshot.centerLoading,
  ]);

  const value = useMemo<LayoutContextType>(
    () => ({
      left: {
        model: snapshot.left,
        setModel: (m) => layoutStore.setSideModel('left', m),
        isLoading: snapshot.leftLoading,
      },
      right: {
        model: snapshot.right,
        setModel: (m) => layoutStore.setSideModel('right', m),
        isLoading: snapshot.rightLoading,
      },
      center: {
        model: snapshot.center,
        setModel: (m) => layoutStore.setSideModel('center', m),
        isLoading: snapshot.centerLoading,
      },
      layoutEpoch: snapshot.version,
      layoutToast: snapshot.layoutToast,
      clearLayoutToast: layoutStore.clearLayoutToast,
      movePanelBetweenSides: layoutStore.movePanelBetweenSides,
      swapFlexSides: layoutStore.swapFlexSides,
      floatPanel: layoutStore.floatPanel,
      dockPanel: layoutStore.dockPanel,
      redockFlexPanelToSide: layoutStore.redockFlexPanelToSide,
      returnAllPanelsToDock: layoutStore.returnAllPanelsToDock,
    }),
    [snapshot],
  );

  return (
    <LayoutContext.Provider value={value}>
      {children}
    </LayoutContext.Provider>
  );
};

// ─── Hooks ───────────────────────────────────────────────────────────────────

export function useLayoutContext(): LayoutContextType {
  const ctx = useContext(LayoutContext);
  if (!ctx) throw new Error('useLayoutContext must be used inside <LayoutProvider>');
  return ctx;
}

/** Convenience hook for a single side. */
export function useSideLayout(side: FlexSide): SideModelState {
  const ctx = useLayoutContext();
  if (side === 'left') return ctx.left;
  if (side === 'right') return ctx.right;
  return ctx.center;
}

/** True when Preview is floated out of the center canvas into a flex-popout. */
export function isPreviewFloating(centerModel: Model | null): boolean {
  if (!centerModel) return false;
  const tab = findTabByComponent(centerModel, 'preview');
  return tab?.isFloating() ?? false;
}
