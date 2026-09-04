/**
 * LayoutContext — provides two FlexLayout Model instances (left + right) to the tree.
 *
 * Responsibilities:
 * - Load each side's JSON from disk via Tauri `load_layout_left` / `load_layout_right`
 * - Parse JSON → Model.fromJson(); graceful fallback to side-specific default
 * - Cross-side panel moves (two models — FlexLayout cannot drag across them)
 * - Expose leftModel / rightModel + setters + per-side loading flag
 *
 * Save-to-disk lives in FlexLayoutContainer (debounced 500 ms, per side).
 */

import React, { createContext, useContext, useEffect, useState, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getAllWebviewWindows } from '@tauri-apps/api/webviewWindow';
import { Actions, DockLocation, Model, TabNode, TabSetNode } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
import { getDefaultFlexLayout, getDefaultFlexLayoutJson, LAYOUT_TOAST_MESSAGES } from '../defaults/DefaultLayouts';
import type { FlexSide } from '../defaults/DefaultLayouts';
import { isPanelOnFlexLayout } from '../factories/layoutPanelFactory';

// ─── Context type ─────────────────────────────────────────────────────────────

export interface SideModelState {
  model: Model | null;
  setModel: (m: Model) => void;
  isLoading: boolean;
}

export interface LayoutContextType {
  left: SideModelState;
  right: SideModelState;
  /**
   * Increments when a side model is mutated in place (float/dock). Consumers that
   * derive docked vs floating must read this so React re-renders — Model identity
   * stays the same on purpose (cloning kills FlexLayout popouts).
   */
  layoutEpoch: number;
  /** One-shot layout migration / recovery message for the shell toast. */
  layoutToast: string | null;
  clearLayoutToast: () => void;
  /** Move a FlexLayout tab (by component id) from one side model to the other. */
  movePanelBetweenSides: (panelComponent: string, to: FlexSide) => void;
  /** Swap left/right FlexLayout models (titlebar sidebar-swap button). */
  swapFlexSides: () => void;
  /** Pop a docked tab into a FlexLayout OS popout window. */
  floatPanel: (side: FlexSide, panelComponent: string) => void;
  /** Dock a floated tab back into its sidebar. */
  dockPanel: (side: FlexSide, panelComponent: string) => void;
  /**
   * Redock a floated FlexLayout panel onto `to` (same side = unfloat,
   * opposite = move + dock). Used by drag-to-redock affinity.
   */
  redockFlexPanelToSide: (panelComponent: string, to: FlexSide) => void;
}

const LayoutContext = createContext<LayoutContextType | undefined>(undefined);

// ─── Model helpers ────────────────────────────────────────────────────────────

export function findTabByComponent(model: Model, component: string): TabNode | null {
  let found: TabNode | null = null;
  model.visitNodes((node) => {
    if (found) return;
    if (node.getType() === TabNode.TYPE) {
      const tab = node as TabNode;
      if (tab.getComponent() === component) found = tab;
    }
  });
  return found;
}

export function findFirstTabSetId(model: Model): string | null {
  let id: string | null = null;
  model.visitNodes((node) => {
    if (id) return;
    if (node.getType() === TabSetNode.TYPE) id = node.getId();
  });
  return id;
}

/** Component ids of FlexLayout tabs currently on this side (including floating). */
export function listFlexComponents(model: Model | null): string[] {
  if (!model) return [];
  const ids: string[] = [];
  model.visitNodes((node) => {
    if (node.getType() !== TabNode.TYPE) return;
    const c = (node as TabNode).getComponent();
    if (c) ids.push(c);
  });
  return ids;
}

/** Component ids of tabs that are still docked in the sidebar (not OS-floated). */
export function listDockedFlexComponents(model: Model | null): string[] {
  if (!model) return [];
  const ids: string[] = [];
  model.visitNodes((node) => {
    if (node.getType() !== TabNode.TYPE) return;
    const tab = node as TabNode;
    if (tab.isFloating()) return;
    const c = tab.getComponent();
    if (c) ids.push(c);
  });
  return ids;
}

/**
 * After float/dock: tabsets that only hold floating tabs collapse so a remaining
 * docked panel fills the column; docked tabsets get normal weight back.
 */
function rebalanceFloatingTabsets(model: Model): void {
  const tabsets: TabSetNode[] = [];
  model.visitNodes((node) => {
    if (node.getType() === TabSetNode.TYPE) tabsets.push(node as TabSetNode);
  });
  for (const ts of tabsets) {
    const children = ts.getChildren();
    let hasDocked = false;
    for (const child of children) {
      if (child.getType() === TabNode.TYPE && !(child as TabNode).isFloating()) {
        hasDocked = true;
        break;
      }
    }
    // Tiny weight keeps the floating tab (and its OS popout) alive in the model
    // without reserving half the dock when a sibling tabset still has content.
    model.doAction(
      Actions.updateNodeAttributes(ts.getId(), { weight: hasDocked ? 100 : 0.01 }),
    );
  }
}

/**
 * Color Lab parity: panels in a sidebar stack vertically — never as side-by-side
 * tabs (full WindowTitlebar chrome cannot share a narrow strip).
 */
export function normalizeSideToVerticalStack(model: Model): boolean {
  let changed = false;
  // Snapshot ids first — visitNodes while mutating is unsafe.
  const tabsetIds: string[] = [];
  model.visitNodes((node) => {
    if (node.getType() === TabSetNode.TYPE) tabsetIds.push(node.getId());
  });

  for (const tsId of tabsetIds) {
    // Loop until this tabset has at most one docked tab.
    for (;;) {
      const ts = model.getNodeById(tsId);
      if (!ts || ts.getType() !== TabSetNode.TYPE) break;
      const docked = (ts as TabSetNode).getChildren().filter((c) => {
        if (c.getType() !== TabNode.TYPE) return false;
        return !(c as TabNode).isFloating();
      }) as TabNode[];
      if (docked.length <= 1) break;
      const move = docked[docked.length - 1]!;
      model.doAction(
        Actions.moveNode(move.getId(), tsId, DockLocation.BOTTOM, -1),
      );
      changed = true;
    }
  }

  if (changed) rebalanceFloatingTabsets(model);
  return changed;
}

// ─── Helper: enforce app chrome policy on any loaded model ────────────────────

/** Tab strip on (styled as WindowTitlebar); float via our menu, not FL toolbar. */
function applyAppChromePolicy(model: Model): Model {
  model.doAction(
    Actions.updateModelAttributes({
      tabSetEnableTabStrip: true,
      tabSetEnableSingleTabStretch: true,
      // Hide FlexLayout's float toolbar button (native browser tooltips).
      // We still call Actions.floatTab from WindowTitlebar / undock drag.
      tabEnableFloat: false,
      tabEnableRename: false,
      tabEnableClose: false,
      tabSetEnableMaximize: false,
      // Invisible overlap splitter — borders of stacked panels share one line.
      splitterSize: 0,
      splitterExtra: 4,
    })
  );
  normalizeSideToVerticalStack(model);
  return model;
}

// ─── Helper: load one side ────────────────────────────────────────────────────

async function loadSideModel(side: FlexSide): Promise<Model> {
  const command = side === 'left' ? 'load_layout_left' : 'load_layout_right';
  let json = getDefaultFlexLayout(side);

  try {
    json = await invoke<string>(command);
  } catch {
    // First run or Tauri unavailable — use default silently.
  }

  try {
    return applyAppChromePolicy(Model.fromJson(JSON.parse(json) as IJsonModel));
  } catch (err) {
    console.warn(`[LayoutProvider] Model.fromJson failed for ${side}, using default:`, err);
    return applyAppChromePolicy(Model.fromJson(getDefaultFlexLayoutJson(side)));
  }
}

function persistSide(side: FlexSide, model: Model): void {
  const command = side === 'left' ? 'save_layout_left' : 'save_layout_right';
  const json = JSON.stringify(model.toJson());
  void invoke<void>(command, { json }).catch((err) => {
    console.error(`[LayoutProvider] ${command} failed:`, err);
  });
}

function tabDisplayName(component: string): string {
  switch (component) {
    case 'layers':
      return 'Layers';
    case 'effect':
      return 'Effect Settings';
    case 'colorlab':
      return 'Color Lab';
    default:
      return component;
  }
}

/** Empty but valid side layout (no tabs) — used after the last tab is moved away. */
function emptySideJson(side: FlexSide): IJsonModel {
  return {
    global: { ...getDefaultFlexLayoutJson(side).global },
    borders: [],
    layout: {
      type: 'row',
      weight: 100,
      children: [
        {
          type: 'tabset',
          weight: 100,
          children: [],
        },
      ],
    },
  };
}

/**
 * B4b: if Color Lab is missing from both side models (saved pre-B4b layout),
 * append it under the right column without replacing the user's Layers/Effect tree.
 */
function ensureColorLabPresent(
  left: Model,
  right: Model,
): { left: Model; right: Model; injected: boolean } {
  if (findTabByComponent(left, 'colorlab') || findTabByComponent(right, 'colorlab')) {
    return { left, right, injected: false };
  }

  const tabJson = {
    type: 'tab',
    name: 'Color Lab',
    component: 'colorlab',
    enableFloat: true,
  };

  const targetTabSetId = findFirstTabSetId(right);
  if (!targetTabSetId) {
    const seeded = applyAppChromePolicy(Model.fromJson(getDefaultFlexLayoutJson('right')));
    return { left, right: seeded, injected: true };
  }

  try {
    right.doAction(Actions.addNode(tabJson, targetTabSetId, DockLocation.BOTTOM, -1, true));
  } catch (err) {
    console.error('[LayoutProvider] failed to inject Color Lab tab:', err);
    return { left, right, injected: false };
  }

  const rightNext = applyAppChromePolicy(Model.fromJson(right.toJson()));
  return { left, right: rightNext, injected: true };
}

// ─── Provider ────────────────────────────────────────────────────────────────

export const LayoutProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [leftModel,  setLeftModelState]  = useState<Model | null>(null);
  const [rightModel, setRightModelState] = useState<Model | null>(null);
  const [leftLoading,  setLeftLoading]  = useState(true);
  const [rightLoading, setRightLoading] = useState(true);
  const [layoutEpoch, setLayoutEpoch] = useState(0);
  const [layoutToast, setLayoutToast] = useState<string | null>(null);

  // Refs so move/float always see latest models without stale closures.
  const leftRef = useRef<Model | null>(null);
  const rightRef = useRef<Model | null>(null);
  leftRef.current = leftModel;
  rightRef.current = rightModel;

  const redockFlexPanelToSideRef = useRef<
    ((panelComponent: string, to: FlexSide) => void) | null
  >(null);

  const bumpLayout = useCallback(() => {
    setLayoutEpoch((n) => n + 1);
  }, []);

  const clearLayoutToast = useCallback(() => {
    setLayoutToast(null);
  }, []);

  const setLeftModel = useCallback((m: Model) => {
    setLeftModelState(m);
    setLayoutEpoch((n) => n + 1);
  }, []);
  const setRightModel = useCallback((m: Model) => {
    setRightModelState(m);
    setLayoutEpoch((n) => n + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [left, right] = await Promise.all([
        loadSideModel('left'),
        loadSideModel('right'),
      ]);
      if (cancelled) return;

      const ensured = ensureColorLabPresent(left, right);
      setLeftModelState(ensured.left);
      setRightModelState(ensured.right);
      setLeftLoading(false);
      setRightLoading(false);

      if (ensured.injected) {
        persistSide('right', ensured.right);
        setLayoutToast(LAYOUT_TOAST_MESSAGES.COLORLAB_ADDED);
      }
    })();
    return () => { cancelled = true; };
  }, []);

  // Drag-to-redock from FlexLayout OS popouts (Rust affinity → this event).
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen<{ panelId: string; side: FlexSide }>(
      'flex-panel-dock-request',
      (event) => {
        if (cancelled) return;
        const { panelId, side } = event.payload;
        if (!isPanelOnFlexLayout(panelId)) return;
        if (side !== 'left' && side !== 'right') return;
        redockFlexPanelToSideRef.current?.(panelId, side);
        void getAllWebviewWindows().then((wins) => {
          void Promise.all(
            wins
              .filter((w) => w.label.startsWith('flex-popout-'))
              .map(async (w) => {
                try {
                  await w.close();
                } catch {
                  /* ignore */
                }
              }),
          );
        });
      },
    ).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const movePanelBetweenSides = useCallback((panelComponent: string, to: FlexSide) => {
    const from: FlexSide = to === 'left' ? 'right' : 'left';
    const fromModel = from === 'left' ? leftRef.current : rightRef.current;
    const toModel = to === 'left' ? leftRef.current : rightRef.current;
    if (!fromModel || !toModel) {
      console.warn('[LayoutContext] movePanelBetweenSides: model missing', { from, to });
      return;
    }

    if (findTabByComponent(toModel, panelComponent)) return;

    const tab = findTabByComponent(fromModel, panelComponent);
    if (!tab) {
      console.warn('[LayoutContext] movePanelBetweenSides: tab not found', panelComponent, 'on', from);
      return;
    }

    // Fresh tab json — drop id/floating so the target model docks a new tab.
    const raw = tab.toJson() as Record<string, unknown>;
    const { id: _omitId, floating: _omitFloat, ...rest } = raw;
    const json = {
      ...rest,
      type: 'tab',
      name: (typeof rest.name === 'string' && rest.name) || tabDisplayName(panelComponent),
      component: panelComponent,
      enableFloat: true,
    };

    fromModel.doAction(Actions.deleteTab(tab.getId()));

    let targetTabSetId = findFirstTabSetId(toModel);
    if (!targetTabSetId) {
      const seed = applyAppChromePolicy(
        Model.fromJson({
          global: getDefaultFlexLayoutJson(to).global,
          borders: [],
          layout: {
            type: 'row',
            weight: 100,
            children: [
              {
                type: 'tabset',
                weight: 100,
                children: [json],
              },
            ],
          },
        } as IJsonModel)
      );
      const fromNext = applyAppChromePolicy(
        Model.fromJson(
          listFlexComponents(fromModel).length === 0
            ? emptySideJson(from)
            : fromModel.toJson()
        )
      );
      if (to === 'left') {
        setLeftModelState(seed);
        setRightModelState(fromNext);
      } else {
        setRightModelState(seed);
        setLeftModelState(fromNext);
      }
      persistSide(to, seed);
      persistSide(from, fromNext);
      return;
    }

    try {
      // Stack below existing panels (Color Lab vertical dock), never as peer tabs.
      toModel.doAction(Actions.addNode(json, targetTabSetId, DockLocation.BOTTOM, -1, true));
    } catch (err) {
      console.error('[LayoutContext] addNode failed', err);
      return;
    }

    normalizeSideToVerticalStack(toModel);
    normalizeSideToVerticalStack(fromModel);

    const fromNext = applyAppChromePolicy(
      Model.fromJson(
        listFlexComponents(fromModel).length === 0
          ? emptySideJson(from)
          : fromModel.toJson()
      )
    );
    const toNext = applyAppChromePolicy(Model.fromJson(toModel.toJson()));

    if (from === 'left') {
      setLeftModelState(fromNext);
      setRightModelState(toNext);
    } else {
      setRightModelState(fromNext);
      setLeftModelState(toNext);
    }
    persistSide(from, fromNext);
    persistSide(to, toNext);
  }, []);

  const floatPanel = useCallback((side: FlexSide, panelComponent: string) => {
    const model = side === 'left' ? leftRef.current : rightRef.current;
    if (!model) {
      console.warn('[LayoutContext] floatPanel: model missing', side);
      return;
    }
    const tab = findTabByComponent(model, panelComponent);
    if (!tab) {
      console.warn('[LayoutContext] floatPanel: tab not found', panelComponent);
      return;
    }
    if (tab.isFloating()) return;
    // Keep the SAME model instance — cloning kills FlexLayout's FloatingWindow.
    model.doAction(Actions.floatTab(tab.getId()));
    // FlexLayout already selects a non-floating sibling in the same tabset;
    // collapse floating-only tabsets so a split sibling fills the dock.
    normalizeSideToVerticalStack(model);
    rebalanceFloatingTabsets(model);
    persistSide(side, model);
    // Model identity unchanged — bump so shell drops reserved dock width.
    bumpLayout();
  }, [bumpLayout]);

  const dockPanel = useCallback((side: FlexSide, panelComponent: string) => {
    const model = side === 'left' ? leftRef.current : rightRef.current;
    if (!model) {
      console.warn('[LayoutContext] dockPanel: model missing', side);
      return;
    }
    const tab = findTabByComponent(model, panelComponent);
    if (!tab) {
      console.warn('[LayoutContext] dockPanel: tab not found', panelComponent);
      return;
    }
    if (!tab.isFloating()) return;

    // Close the browser/Tauri popout shell if FlexLayout still has a Window handle.
    try {
      const pop = tab.getWindow?.();
      if (pop && !pop.closed) pop.close();
    } catch {
      /* ignore */
    }

    model.doAction(Actions.unFloatTab(tab.getId()));
    normalizeSideToVerticalStack(model);
    rebalanceFloatingTabsets(model);
    persistSide(side, model);
    bumpLayout();
  }, [bumpLayout]);

  const redockFlexPanelToSide = useCallback(
    (panelComponent: string, to: FlexSide) => {
      const leftM = leftRef.current;
      const rightM = rightRef.current;
      const onLeft = leftM ? findTabByComponent(leftM, panelComponent) : null;
      const onRight = rightM ? findTabByComponent(rightM, panelComponent) : null;
      const from: FlexSide | null = onLeft?.isFloating()
        ? 'left'
        : onRight?.isFloating()
          ? 'right'
          : null;
      if (!from) return;

      if (from === to) {
        dockPanel(from, panelComponent);
      } else {
        // movePanelBetweenSides strips `floating` and docks into the target side.
        movePanelBetweenSides(panelComponent, to);
      }
    },
    [dockPanel, movePanelBetweenSides],
  );
  redockFlexPanelToSideRef.current = redockFlexPanelToSide;

  const swapFlexSides = useCallback(() => {
    const left = leftRef.current;
    const right = rightRef.current;
    if (!left || !right) {
      console.warn('[LayoutContext] swapFlexSides: model missing');
      return;
    }
    // Exchange models as-is (including floating tabs / OS popouts).
    leftRef.current = right;
    rightRef.current = left;
    setLeftModelState(right);
    setRightModelState(left);
    persistSide('left', right);
    persistSide('right', left);
    bumpLayout();
  }, [bumpLayout]);

  const value: LayoutContextType = {
    left:  { model: leftModel,  setModel: setLeftModel,  isLoading: leftLoading  },
    right: { model: rightModel, setModel: setRightModel, isLoading: rightLoading },
    layoutEpoch,
    layoutToast,
    clearLayoutToast,
    movePanelBetweenSides,
    swapFlexSides,
    floatPanel,
    dockPanel,
    redockFlexPanelToSide,
  };

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
  return side === 'left' ? ctx.left : ctx.right;
}
