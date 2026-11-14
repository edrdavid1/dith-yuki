import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useShell } from './shell/ShellContext';
import {
  isPreviewFloating,
  listDockedFlexComponents,
  listFlexComponents,
  listFloatingFlexComponents,
  useLayoutContext,
} from '../contexts/LayoutContext';
import { isPanelOnFlexLayout } from '../factories/layoutPanelFactory';
import {
  forgetFlexPopoutLabelByWindowLabel,
  lookupPanelIdByLabel,
} from '../features/panels/flexPopoutRegistry';
import {
  SIDEBAR_COLLAPSE_WIDTH,
  SIDEBAR_EXPANDED_MAX_WIDTH,
  SIDEBAR_EXPANDED_MIN_WIDTH,
} from '../defaults/DefaultLayouts';
import { maxDockedHorizontalSpan } from '../features/panels/horizontalDock';
import { computeShellWidths } from '../features/panels/computeShellWidths';
import type { DockSide, PanelId } from '../types/panels';
import { appEdgesAttr } from '../shared/ui/appEdges';
import { isMacOS } from '../lib/platform';
import { onDockAffinity, type DockAffinityEvent } from '../shared/ipc';

/**
 * Sidebar widths, collapse, and the FlexLayout host flags for the main shell.
 * Emergency collapse is visual only and is never written back to prefs.
 */
export function useDockFrame(focusMode: boolean) {
  const {
    leftSidebar,
    rightSidebar,
    setSidebarCollapsed,
    setSidebarWidth,
    swapSidebars,
    previewBackground,
  } = useShell();
  const {
    left: leftLayout,
    right: rightLayout,
    center: centerLayout,
    layoutEpoch,
    swapFlexSides,
    layoutToast,
    layoutToastEpoch,
    clearLayoutToast,
    dockPanel,
    returnAllPanelsToDock,
  } = useLayoutContext();
  // layoutEpoch: re-read docked vs floating after in-place float/dock mutations.
  void layoutEpoch;

  const [affinity, setAffinity] = useState<DockAffinityEvent | null>(null);
  const leftHitRef = useRef<HTMLElement | null>(null);
  const rightHitRef = useRef<HTMLElement | null>(null);

  const leftFlexIds = listFlexComponents(leftLayout.model).filter(
    (id): id is PanelId => isPanelOnFlexLayout(id)
  );
  const rightFlexIds = listFlexComponents(rightLayout.model).filter(
    (id): id is PanelId => isPanelOnFlexLayout(id)
  );
  const leftDockedFlexIds = listDockedFlexComponents(leftLayout.model).filter(
    (id): id is PanelId => isPanelOnFlexLayout(id)
  );
  const rightDockedFlexIds = listDockedFlexComponents(rightLayout.model).filter(
    (id): id is PanelId => isPanelOnFlexLayout(id)
  );
  const previewFloating = isPreviewFloating(centerLayout.model);

  const leftHasFlex = leftFlexIds.length > 0;
  const rightHasFlex = rightFlexIds.length > 0;
  const leftHasDockedFlex = leftDockedFlexIds.length > 0;
  const rightHasDockedFlex = rightDockedFlexIds.length > 0;
  const leftFlexOnly = leftHasDockedFlex;
  const rightFlexOnly = rightHasDockedFlex;
  const leftFloatHostOnly = leftHasFlex && !leftHasDockedFlex;
  const rightFloatHostOnly = rightHasFlex && !rightHasDockedFlex;

  const [viewportW, setViewportW] = useState(
    () => (typeof window !== 'undefined' ? window.innerWidth : 1280),
  );
  useEffect(() => {
    const onResize = () => setViewportW(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  const leftSpan = maxDockedHorizontalSpan(leftLayout.model);
  const rightSpan = maxDockedHorizontalSpan(rightLayout.model);
  const shell = computeShellWidths({
    viewportWidth: viewportW,
    prefs: { leftWidth: leftSidebar.width, rightWidth: rightSidebar.width },
    columnsLeft: leftHasDockedFlex && !focusMode ? leftSpan : 0,
    columnsRight: rightHasDockedFlex && !focusMode ? rightSpan : 0,
    collapsed: {
      left: focusMode || !leftHasDockedFlex || leftSidebar.collapsed,
      right: focusMode || !rightHasDockedFlex || rightSidebar.collapsed,
    },
  });
  const leftW = focusMode ? 0 : shell.leftWidth;
  const rightW = focusMode ? 0 : shell.rightWidth;
  const leftVisuallyCollapsed =
    focusMode || leftSidebar.collapsed || shell.emergencyCollapseLeft;
  const rightVisuallyCollapsed =
    focusMode || rightSidebar.collapsed || shell.emergencyCollapseRight;

  const leftAppEdges = isMacOS() ? appEdgesAttr('bottom', 'left') : undefined;
  const rightAppEdges = isMacOS() ? appEdgesAttr('bottom', 'right') : undefined;
  const previewAppEdges = isMacOS()
    ? appEdgesAttr('bottom', leftW === 0 && 'left', rightW === 0 && 'right')
    : undefined;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void onDockAffinity((event) => {
      if (cancelled) return;
      const payload = event.payload;
      if (!payload.armed) {
        setAffinity(null);
        return;
      }
      setAffinity(payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen<{ panelId: string; side: 'left' | 'right' }>(
      'flex-panel-dock-request',
      (event) => {
        if (cancelled) return;
        const { side } = event.payload;
        if (side === 'left' || side === 'right') {
          setSidebarCollapsed(side, false);
        }
      },
    ).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [setSidebarCollapsed]);

  // W2: OS-closed flex-popout → dock the matching floating tab.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen<{ label: string }>('flex-popout-destroyed', (event) => {
      if (cancelled) return;
      const label = event.payload?.label;
      if (!label) return;
      const panelId = lookupPanelIdByLabel(label);
      forgetFlexPopoutLabelByWindowLabel(label);
      if (!panelId || !isPanelOnFlexLayout(panelId)) {
        console.warn('[flex-dock] flex-popout-destroyed: no panel for', label);
        return;
      }
      console.info('[flex-dock] flex-popout-destroyed → dock', panelId, label);
      if (panelId === 'preview') {
        dockPanel('center', 'preview');
        return;
      }
      const onLeft = listFloatingFlexComponents(leftLayout.model).includes(panelId);
      const onRight = listFloatingFlexComponents(rightLayout.model).includes(panelId);
      if (onLeft) dockPanel('left', panelId);
      else if (onRight) dockPanel('right', panelId);
      else {
        console.info('[flex-dock] destroyed panel already non-floating', panelId);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [dockPanel, leftLayout.model, rightLayout.model]);

  const resizeColumnWidth = useCallback(
    (side: DockSide, delta: number, allowCollapse: boolean) => {
      setSidebarWidth(side, (w) => {
        const signedDelta = side === 'right' ? -delta : delta;
        const newW = w + signedDelta;
        if (allowCollapse && newW < SIDEBAR_COLLAPSE_WIDTH) {
          requestAnimationFrame(() => setSidebarCollapsed(side, true));
          return w;
        }
        return Math.min(
          SIDEBAR_EXPANDED_MAX_WIDTH,
          Math.max(SIDEBAR_EXPANDED_MIN_WIDTH, newW),
        );
      });
    },
    [setSidebarWidth, setSidebarCollapsed],
  );

  const hasFloatingPanels =
    listFloatingFlexComponents(leftLayout.model).length > 0 ||
    listFloatingFlexComponents(rightLayout.model).length > 0 ||
    listFloatingFlexComponents(centerLayout.model).length > 0;

  return {
    affinity,
    leftHitRef,
    rightHitRef,
    leftDockedFlexIds,
    rightDockedFlexIds,
    previewFloating,
    leftFlexOnly,
    rightFlexOnly,
    leftFloatHostOnly,
    rightFloatHostOnly,
    leftW,
    rightW,
    leftVisuallyCollapsed,
    rightVisuallyCollapsed,
    leftAppEdges,
    rightAppEdges,
    previewAppEdges,
    previewBackground,
    setSidebarCollapsed,
    swapSidebars,
    swapFlexSides,
    layoutToast,
    layoutToastEpoch,
    clearLayoutToast,
    dockPanel,
    returnAllPanelsToDock,
    resizeColumnWidth,
    hasFloatingPanels,
    gridTemplateColumns: `${leftW}px 1fr ${rightW}px`,
  };
}
