import { useEffect, useRef } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { updateDockZone, type DockZonePayload } from '../shared/ipc/panels';

export interface UseDockZoneReporterOptions {
  /** Expanded sidebar element (may be display:none when collapsed). */
  sidebarRef: React.RefObject<HTMLElement | null>;
  /** Collapsed icon strip (present when sidebarCollapsed). */
  collapsedRef?: React.RefObject<HTMLElement | null>;
  /**
   * Canvas-edge drop rail used when the side has no docked panels.
   * Preferred hit geometry for float→dock onto an empty side.
   */
  emptyEdgeRef?: React.RefObject<HTMLElement | null>;
  sidebarSide: 'left' | 'right';
  sidebarCollapsed: boolean;
  /** Logical width used when DOM target is missing but dock targets exist. */
  sidebarWidth: number;
  /** When false, clears the dock zone (no dockable targets). */
  hasDockTargets: boolean;
  /**
   * When true and the sidebar column is empty (width 0 / no DOM), still report a
   * thin edge strip so the first panel can redock onto an empty side.
   */
  reportEmptyEdge?: boolean;
}

/**
 * Reports sidebar screen geometry + slot midpoints to Rust for dock affinity.
 * Coalesces updates to one IPC call per animation frame.
 */
export function useDockZoneReporter(options: UseDockZoneReporterOptions): void {
  const latestRef = useRef(options);
  latestRef.current = options;
  const rafRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;

    const flush = async () => {
      rafRef.current = null;
      if (cancelled) return;

      const {
        sidebarRef: sb,
        collapsedRef: col,
        emptyEdgeRef: emptyEdge,
        sidebarSide: side,
        sidebarCollapsed: collapsed,
        sidebarWidth: widthPref,
        hasDockTargets: hasTargets,
        reportEmptyEdge = true,
      } = latestRef.current;

      if (!hasTargets && !reportEmptyEdge) {
        try {
          await updateDockZone(side, null);
        } catch {
          /* ignore */
        }
        return;
      }

      try {
        const win = getCurrentWindow();
        const scale = await win.scaleFactor();
        const innerPos = await win.innerPosition();
        const innerSize = await win.innerSize();
        const innerLogicalX = innerPos.x / scale;
        const innerLogicalY = innerPos.y / scale;
        const innerW = innerSize.width / scale;
        const innerH = innerSize.height / scale;

        // Prefer empty-edge rail when vacant; else collapsed strip / expanded sidebar.
        const el = !hasTargets
          ? emptyEdge?.current ?? null
          : collapsed
            ? col?.current ?? null
            : sb.current;
        let x: number;
        let y: number;
        let width: number;
        let height: number;
        const slots: { midY: number; top: number; bottom: number }[] = [];

        if (el) {
          const rect = el.getBoundingClientRect();
          // display:none → zero rect; fall back to geometry estimate.
          if (rect.width > 0 && rect.height > 0) {
            x = innerLogicalX + rect.left;
            y = innerLogicalY + rect.top;
            width = rect.width;
            height = rect.height;
            el.querySelectorAll<HTMLElement>('[data-panel-id]').forEach((panelEl) => {
              const pr = panelEl.getBoundingClientRect();
              if (pr.height <= 0) return;
              const top = innerLogicalY + pr.top;
              const bottom = top + pr.height;
              slots.push({ midY: top + pr.height / 2, top, bottom });
            });
          } else {
            const stripW = !hasTargets
              ? 64
              : collapsed
                ? 40
                : Math.max(40, widthPref);
            width = stripW;
            height = innerH;
            y = innerLogicalY;
            x = side === 'right' ? innerLogicalX + innerW - stripW : innerLogicalX;
          }
        } else {
          // Ref not ready / empty side — estimate edge strip for affinity.
          const stripW = !hasTargets
            ? 64
            : collapsed
              ? 40
              : Math.max(40, widthPref);
          width = stripW;
          height = innerH;
          y = innerLogicalY;
          x = side === 'right' ? innerLogicalX + innerW - stripW : innerLogicalX;
          if (retryTimer == null && (hasTargets || reportEmptyEdge)) {
            retryTimer = setTimeout(() => {
              retryTimer = null;
              schedule();
            }, 100);
          }
        }

        const payload: DockZonePayload = {
          x,
          y,
          width,
          height,
          scaleFactor: scale,
          side,
          slots,
        };
        await updateDockZone(side, payload);
      } catch (err) {
        console.warn('[useDockZoneReporter] update_dock_zone failed', err);
      }
    };

    const schedule = () => {
      if (cancelled) return;
      if (rafRef.current != null) return;
      rafRef.current = requestAnimationFrame(() => {
        void flush();
      });
    };

    schedule();

    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(() => schedule()) : null;
    const observeEl = () => {
      ro?.disconnect();
      const latest = latestRef.current;
      const target = !latest.hasDockTargets
        ? latest.emptyEdgeRef?.current
        : latest.sidebarCollapsed
          ? latest.collapsedRef?.current
          : latest.sidebarRef.current;
      if (target) ro?.observe(target);
    };
    observeEl();
    // Refs can populate a tick later.
    const observeTimer = setTimeout(observeEl, 50);

    window.addEventListener('resize', schedule);

    let unlistenRefresh: { current: (() => void) | null } = { current: null };
    void import('@tauri-apps/api/event').then(({ listen }) => {
      if (cancelled) return;
      void listen('dock-zones-refresh', () => {
        schedule();
      }).then((fn) => {
        if (cancelled) fn();
        else unlistenRefresh.current = fn;
      });
    });

    let unlistenMoved: (() => void) | null = null;
    let unlistenResized: (() => void) | null = null;
    const win = getCurrentWindow();
    win.onMoved(() => schedule()).then((fn) => {
      if (cancelled) fn();
      else unlistenMoved = fn;
    });
    win.onResized(() => schedule()).then((fn) => {
      if (cancelled) fn();
      else unlistenResized = fn;
    });

    return () => {
      cancelled = true;
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
      if (retryTimer != null) clearTimeout(retryTimer);
      clearTimeout(observeTimer);
      ro?.disconnect();
      window.removeEventListener('resize', schedule);
      unlistenRefresh.current?.();
      unlistenMoved?.();
      unlistenResized?.();
      // Do NOT clear zone here — Strict Mode / dep churn was wiping the zone
      // before the next flush, leaving affinity permanently disarmed.
    };
  }, [
    options.sidebarSide,
    options.sidebarCollapsed,
    // width is read from latestRef; ResizeObserver covers live drag without rebinding.
    options.hasDockTargets,
    options.reportEmptyEdge,
  ]);
}
