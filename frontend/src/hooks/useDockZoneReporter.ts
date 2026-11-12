import { useEffect, useRef } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { updateDockZone, type DockZonePayload } from '../shared/ipc/panels';
import {
  cssRectToPhysicalScreen,
  physicalEdgeStrip,
} from '../features/panels/dockZoneCoords';

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
  /** When true, clears zones and skips reporting (center Preview host). */
  disabled?: boolean;
}

/**
 * Reports sidebar screen geometry + slot midpoints to Rust for dock affinity.
 * W6: physical screen pixels + monotonic seq; at most one IPC per animation frame.
 */
export function useDockZoneReporter(options: UseDockZoneReporterOptions): void {
  const latestRef = useRef(options);
  latestRef.current = options;
  const rafRef = useRef<number | null>(null);
  const seqRef = useRef(0);

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
        disabled = false,
      } = latestRef.current;

      if (disabled) {
        return;
      }

      if (!hasTargets && !reportEmptyEdge) {
        try {
          await updateDockZone(side, null);
        } catch (err) {
          console.error('[useDockZoneReporter] clear zone failed', err);
        }
        return;
      }

      try {
        const win = getCurrentWindow();
        const scale = await win.scaleFactor();
        // Physical origin of the webview client area.
        const innerPos = await win.innerPosition();
        const innerSize = await win.innerSize();
        const origin = {
          physicalX: innerPos.x,
          physicalY: innerPos.y,
          dpr: scale,
        };

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
          if (rect.width > 0 && rect.height > 0) {
            const phys = cssRectToPhysicalScreen(
              {
                left: rect.left,
                top: rect.top,
                width: rect.width,
                height: rect.height,
              },
              origin,
            );
            x = phys.x;
            y = phys.y;
            width = phys.width;
            height = phys.height;
            el.querySelectorAll<HTMLElement>('[data-panel-id]').forEach((panelEl) => {
              const pr = panelEl.getBoundingClientRect();
              if (pr.height <= 0) return;
              const p = cssRectToPhysicalScreen(
                {
                  left: pr.left,
                  top: pr.top,
                  width: pr.width,
                  height: pr.height,
                },
                origin,
              );
              slots.push({
                midY: p.y + p.height / 2,
                top: p.y,
                bottom: p.y + p.height,
              });
            });
          } else {
            const stripW = !hasTargets ? 64 : collapsed ? 40 : Math.max(40, widthPref);
            const strip = physicalEdgeStrip(
              side,
              origin,
              innerSize.width,
              innerSize.height,
              stripW,
            );
            x = strip.x;
            y = strip.y;
            width = strip.width;
            height = strip.height;
          }
        } else {
          const stripW = !hasTargets ? 64 : collapsed ? 40 : Math.max(40, widthPref);
          const strip = physicalEdgeStrip(
            side,
            origin,
            innerSize.width,
            innerSize.height,
            stripW,
          );
          x = strip.x;
          y = strip.y;
          width = strip.width;
          height = strip.height;
          if (retryTimer == null && (hasTargets || reportEmptyEdge)) {
            retryTimer = setTimeout(() => {
              retryTimer = null;
              schedule();
            }, 100);
          }
        }

        seqRef.current += 1;
        const payload: DockZonePayload = {
          x,
          y,
          width,
          height,
          scaleFactor: scale,
          side,
          slots,
          seq: seqRef.current,
        };
        await updateDockZone(side, payload);
      } catch (err) {
        console.error('[useDockZoneReporter] update_dock_zone failed', err);
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
    };
  }, [
    options.sidebarSide,
    options.sidebarCollapsed,
    options.hasDockTargets,
    options.reportEmptyEdge,
  ]);
}
