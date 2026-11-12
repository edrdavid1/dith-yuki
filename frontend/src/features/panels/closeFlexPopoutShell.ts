/**
 * Close one FlexLayout OS popout without touching siblings.
 *
 * W6: `flexPopoutRegistry` is SoT. If a preferred label / panelId does not
 * resolve to a live window, cancel with a log (+ toast) — never fall back to
 * “any / first / focused” shell.
 */

import { getAllWebviewWindows } from '@tauri-apps/api/webviewWindow';
import type { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import {
  listBoundFlexPopoutLabels,
  lookupFlexPopoutLabel,
} from './flexPopoutRegistry';

const FLEX_POPOUT_PREFIX = 'flex-popout-';
/**
 * Logical CSS-px tolerance when matching DOM screenX/Y to Tauri outer/scale.
 * Keep loose: WKWebView screenX can lag setPosition by a few dozen px.
 */
const POS_MATCH_PX = 48;

export type DomWindowScreenPos = {
  screenX: number;
  screenY: number;
};

export type TauriOuterLogicalPos = {
  label: string;
  x: number;
  y: number;
};

/** Pure: pick the Tauri popout whose outer origin matches the DOM window. */
export function matchFlexPopoutLabelByPosition(
  dom: DomWindowScreenPos,
  candidates: TauriOuterLogicalPos[],
  tolerancePx: number = POS_MATCH_PX,
): string | null {
  if (candidates.length === 0) return null;
  let best: TauriOuterLogicalPos | null = null;
  let bestDist = Infinity;
  for (const c of candidates) {
    const dx = c.x - dom.screenX;
    const dy = c.y - dom.screenY;
    const dist = Math.hypot(dx, dy);
    if (dist <= tolerancePx && dist < bestDist) {
      best = c;
      bestDist = dist;
    }
  }
  // W6: no sole-candidate fallback — ambiguous / miss → null.
  return best?.label ?? null;
}

async function listFlexPopoutWindows(): Promise<WebviewWindow[]> {
  const wins = await getAllWebviewWindows();
  return wins.filter((w) => w.label.startsWith(FLEX_POPOUT_PREFIX));
}

/**
 * Logical CSS-px outer origin — same space as `window.screenX/Y` and the
 * drag path (`outerPosition / scaleFactor`).
 */
async function outerLogicalPos(
  w: WebviewWindow,
): Promise<TauriOuterLogicalPos | null> {
  try {
    const scale = await w.scaleFactor();
    const pos = await w.outerPosition();
    const s = scale > 0 ? scale : 1;
    return {
      label: w.label,
      x: pos.x / s,
      y: pos.y / s,
    };
  } catch (err) {
    console.error('[closeFlexPopoutShell] outerPosition failed', w.label, err);
    return null;
  }
}

async function findByLabel(label: string): Promise<WebviewWindow | null> {
  const flex = await listFlexPopoutWindows();
  return flex.find((w) => w.label === label) ?? null;
}

export type FindFlexPopoutOptions = {
  /** Panel component id — registry lookup before position matching. */
  panelId?: string | null;
  /**
   * Drag init only: when exactly one live `flex-popout-*` exists, bind it.
   * Close path must keep this false (W6: no any/first close).
   */
  allowSoleCandidate?: boolean;
};

/** Resolve the `flex-popout-*` handle for a popout browsing context. */
export async function findFlexPopoutWebviewForDomWindow(
  domWin: Window,
  options?: FindFlexPopoutOptions,
): Promise<WebviewWindow | null> {
  const preferred = lookupFlexPopoutLabel(options?.panelId, domWin);
  if (preferred) {
    const byLabel = await findByLabel(preferred);
    if (byLabel) return byLabel;
    console.error(
      '[flex-dock] registry label not live for DOM window',
      preferred,
      options?.panelId ?? null,
    );
    // Fall through to position / sole-candidate — stale registry must not
    // permanently block drag.
  }

  const flex = await listFlexPopoutWindows();
  if (flex.length === 0) return null;

  // Prefer shells not yet claimed by another panel (2nd/3rd float).
  const bound = listBoundFlexPopoutLabels();
  const preferredLabel = lookupFlexPopoutLabel(options?.panelId, null);
  if (preferredLabel) bound.delete(preferredLabel); // our own claim is fine
  const unbound = flex.filter((w) => !bound.has(w.label));

  if (options?.allowSoleCandidate) {
    if (unbound.length === 1) return unbound[0]!;
    if (flex.length === 1) return flex[0]!;
  }

  const pool = unbound.length > 0 ? unbound : flex;
  const positions: TauriOuterLogicalPos[] = [];
  for (const w of pool) {
    const p = await outerLogicalPos(w);
    if (p) positions.push(p);
  }

  const label = matchFlexPopoutLabelByPosition(
    { screenX: domWin.screenX, screenY: domWin.screenY },
    positions,
  );
  if (!label) {
    // Last resort for drag/mount: newest unbound shell (highest flex-popout-N).
    if (options?.allowSoleCandidate && unbound.length > 0) {
      const sorted = [...unbound].sort((a, b) =>
        a.label.localeCompare(b.label, undefined, { numeric: true }),
      );
      return sorted[sorted.length - 1]!;
    }
    console.error('[flex-dock] popout position match failed', {
      panelId: options?.panelId ?? null,
      domScreen: { x: domWin.screenX, y: domWin.screenY },
      candidates: positions,
      flexCount: flex.length,
      unboundCount: unbound.length,
    });
    return null;
  }
  return flex.find((w) => w.label === label) ?? null;
}

export type CloseFlexPopoutOptions = {
  /** Prefer this `flex-popout-*` label when closing. */
  preferredLabel?: string | null;
  /** Panel component id — looked up in the registry when label is unknown. */
  panelId?: string | null;
  /** Optional toast hook (W6 label-miss). */
  onResolveFailed?: (message: string) => void;
};

/**
 * Close the OS shell for one popout. Prefer explicit label / registry.
 * W6: if a preferred label does not resolve, cancel — no any/first fallback.
 */
export async function closeFlexPopoutShell(
  domWin?: Window | null,
  options?: CloseFlexPopoutOptions,
): Promise<void> {
  const preferred =
    options?.preferredLabel ??
    lookupFlexPopoutLabel(options?.panelId, domWin ?? null);

  let tauriWin: WebviewWindow | null = null;

  if (preferred) {
    tauriWin = await findByLabel(preferred);
    if (!tauriWin) {
      const msg = `Could not find popout window ${preferred}`;
      console.error('[flex-dock] closeFlexPopoutShell: label unresolved', {
        preferredLabel: preferred,
        panelId: options?.panelId ?? null,
      });
      options?.onResolveFailed?.(msg);
      try {
        const { layoutStore } = await import('../../contexts/layoutStore');
        layoutStore.setLayoutToast(msg);
      } catch {
        /* store optional during tests */
      }
      // Still try to close the DOM proxy so the portal unmounts.
      if (domWin && !domWin.closed) {
        try {
          domWin.close();
        } catch {
          /* ignore */
        }
      }
      return;
    }
  }

  if (!tauriWin && domWin && !domWin.closed) {
    tauriWin = await findFlexPopoutWebviewForDomWindow(domWin, {
      panelId: options?.panelId,
      // Never sole-candidate on close — would risk closing a sibling.
      allowSoleCandidate: false,
    });
  }

  if (domWin && !domWin.closed) {
    try {
      domWin.close();
    } catch {
      /* ignore */
    }
  }

  if (!tauriWin) {
    if (preferred || options?.panelId) {
      const msg = 'Could not resolve floating panel window';
      console.error('[flex-dock] closeFlexPopoutShell: unresolved', {
        preferredLabel: preferred ?? null,
        panelId: options?.panelId ?? null,
      });
      options?.onResolveFailed?.(msg);
    }
    return;
  }

  try {
    await tauriWin.close();
  } catch (err) {
    console.error('[closeFlexPopoutShell] WebviewWindow.close failed:', err);
  }
}
