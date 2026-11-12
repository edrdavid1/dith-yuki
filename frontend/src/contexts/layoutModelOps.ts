/**
 * Pure FlexLayout model helpers (no Model.fromJson ownership).
 * Model construction lives in layoutStore.ts (W4).
 */

import { Actions, DockLocation, Model, TabNode, TabSetNode } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
import {
  DOCK_TABSET_MIN_HEIGHT,
  DOCK_TABSET_MIN_WIDTH,
  getDefaultFlexLayoutJson,
} from '../defaults/DefaultLayouts';
import type { FlexSide } from '../defaults/DefaultLayouts';

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

/** Component ids of OS-floated tabs on this side. */
export function listFloatingFlexComponents(model: Model | null): string[] {
  if (!model) return [];
  const ids: string[] = [];
  model.visitNodes((node) => {
    if (node.getType() !== TabNode.TYPE) return;
    const tab = node as TabNode;
    if (!tab.isFloating()) return;
    const c = tab.getComponent();
    if (c) ids.push(c);
  });
  return ids;
}

/**
 * W2: force-dock every floating tab in `model` (no OS windows reopened).
 * Returns how many tabs were unfloated.
 */
export function dockAllFloating(model: Model): number {
  const floating: TabNode[] = [];
  model.visitNodes((node) => {
    if (node.getType() !== TabNode.TYPE) return;
    const tab = node as TabNode;
    if (tab.isFloating()) floating.push(tab);
  });
  if (floating.length === 0) return 0;

  for (const tab of floating) {
    model.doAction(Actions.unFloatTab(tab.getId()));
  }
  normalizeSideToVerticalStack(model);
  rebalanceFloatingTabsets(model);
  console.info(
    `[flex-dock] dockAllFloating: docked ${floating.length} tab(s)`,
    floating.map((t) => t.getComponent()).filter(Boolean),
  );
  return floating.length;
}

/**
 * After float/dock: collapse ghost hosts so a remaining docked panel fills the column.
 *
 * Floating tabs must stay in the model (OS popouts), but a floating-only *tabset*
 * still occupies a stack slot — tiny weight still gets ~1px from FlexLayout's fill
 * loop, and the title strip paints as the residual gap. Parking floating tabs into
 * a docked sibling tabset lets `_tidy` delete the empty host so one tabset owns 100%.
 */
export function rebalanceFloatingTabsets(model: Model): void {
  if (rebalanceGuard > 0) return;
  rebalanceGuard += 1;
  try {
    const tabsets: TabSetNode[] = [];
    model.visitNodes((node) => {
      if (node.getType() === TabSetNode.TYPE) tabsets.push(node as TabSetNode);
    });

    const dockedHosts: TabSetNode[] = [];
    const floatingOnly: TabSetNode[] = [];
    for (const ts of tabsets) {
      let hasDocked = false;
      let hasFloating = false;
      for (const child of ts.getChildren()) {
        if (child.getType() !== TabNode.TYPE) continue;
        if ((child as TabNode).isFloating()) hasFloating = true;
        else hasDocked = true;
      }
      if (hasDocked) dockedHosts.push(ts);
      else if (hasFloating) floatingOnly.push(ts);
    }

    // Park floating-only hosts into a docked sibling — empty tabsets are tidied away.
    if (dockedHosts.length > 0 && floatingOnly.length > 0) {
      const hostId = dockedHosts[0]!.getId();
      for (const ts of floatingOnly) {
        // Snapshot — moveNode mutates children.
        const movers = ts
          .getChildren()
          .filter(
            (c) => c.getType() === TabNode.TYPE && (c as TabNode).isFloating(),
          ) as TabNode[];
        for (const tab of movers) {
          model.doAction(
            Actions.moveNode(tab.getId(), hostId, DockLocation.CENTER, -1),
          );
        }
      }
    }

    // Refresh tabset list after parking/tidy; ensure a docked tab is selected.
    const after: TabSetNode[] = [];
    model.visitNodes((node) => {
      if (node.getType() === TabSetNode.TYPE) after.push(node as TabSetNode);
    });
    for (const ts of after) {
      const children = ts.getChildren();
      let hasDocked = false;
      let dockedTab: TabNode | null = null;
      for (const child of children) {
        if (child.getType() !== TabNode.TYPE) continue;
        const tab = child as TabNode;
        if (!tab.isFloating()) {
          hasDocked = true;
          if (!dockedTab) dockedTab = tab;
        }
      }
      if (hasDocked && dockedTab) {
        const selected = ts.getSelectedNode();
        if (
          !selected ||
          selected.getType() !== TabNode.TYPE ||
          (selected as TabNode).isFloating()
        ) {
          model.doAction(Actions.selectTab(dockedTab.getId()));
        }
      }
      // Fallback for all-floating column (sidebar already visually collapsed):
      // fixed height 0 so nothing reserves a titlebar strip.
      // Do NOT reset weight on docked tabsets — that fights the splitter.
      // Floating-only hosts also get enableTabStrip false; CSS targets
      // [data-flex-float-host] markers from onRenderTabSet as a belt-and-braces
      // kill for residual FlexLayout fill-loop chrome.
      model.doAction(
        Actions.updateNodeAttributes(
          ts.getId(),
          hasDocked
            ? {
                minHeight: DOCK_TABSET_MIN_HEIGHT,
                // Hard floor for side-by-side splitter drag (≥240 each).
                minWidth: DOCK_TABSET_MIN_WIDTH,
                height: undefined,
                enableTabStrip: true,
              }
            : {
                weight: 0.01,
                minHeight: 0,
                minWidth: 0,
                height: 0,
                enableTabStrip: false,
              },
        ),
      );
    }
  } finally {
    rebalanceGuard -= 1;
  }
}

/** Prevent re-entrant rebalance when moveNode/selectTab fires onModelChange. */
let rebalanceGuard = 0;

/**
 * WindowTitlebar chrome cannot share a tab strip — if a drop created peer
 * docked tabs in one tabset, stack the extras below. Horizontal (left/right)
 * splits into separate tabsets are left alone.
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

/** Tab strip on (styled as WindowTitlebar); float via our menu, not FL toolbar. */
export function applyAppChromePolicy(model: Model): Model {
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
      // Invisible overlap splitter — borders of stacked/side-by-side panels share one line.
      splitterSize: 0,
      splitterExtra: 4,
      // Left/right edge drops create side-by-side tabsets inside the column.
      enableEdgeDock: true,
      // Titlebar-tall floor so stacked panels cannot cover each other's titles.
      tabSetMinHeight: DOCK_TABSET_MIN_HEIGHT,
      tabSetTabStripHeight: DOCK_TABSET_MIN_HEIGHT,
      // Usable floor for side-by-side (and single) docked panels.
      tabSetMinWidth: DOCK_TABSET_MIN_WIDTH,
    }),
  );
  normalizeSideToVerticalStack(model);
  // Always rebalance — normalize may no-op on an already-vertical saved layout.
  rebalanceFloatingTabsets(model);
  return model;
}

export function tabDisplayName(component: string): string {
  switch (component) {
    case 'layers':
      return 'Layers';
    case 'effect':
      return 'Effect Settings';
    case 'colorlab':
      return 'Color Lab';
    case 'preview':
      return 'Preview';
    default:
      return component;
  }
}

export function modelForSide(
  side: FlexSide,
  left: Model | null,
  right: Model | null,
  center: Model | null,
): Model | null {
  if (side === 'left') return left;
  if (side === 'right') return right;
  return center;
}

/** Empty but valid side layout (no tabs) — used after the last tab is moved away. */
export function emptySideJson(side: FlexSide): IJsonModel {
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
