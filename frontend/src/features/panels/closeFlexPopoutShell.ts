/**
 * Close one FlexLayout OS popout without touching siblings.
 *
 * Browser `Window.close()` on the `window.open` proxy is unreliable in Tauri /
 * WKWebView — the real shell is a `flex-popout-*` WebviewWindow. Match that
 * handle by outer position (while the DOM window is still open), then close
 * both. Never close-all: multi-float siblings must stay up.
 */

import { getAllWebviewWindows } from '@tauri-apps/api/webviewWindow';
import type { WebviewWindow } from '@tauri-apps/api/webviewWindow';

const FLEX_POPOUT_PREFIX = 'flex-popout-';
/** Logical-px tolerance when matching DOM screenX/Y to Tauri outerPosition. */
const POS_MATCH_PX = 8;

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
  if (best) return best.label;
  // Single orphaned popout — safe to close without a position hit.
  if (candidates.length === 1) return candidates[0]!.label;
  return null;
}

async function listFlexPopoutWindows(): Promise<WebviewWindow[]> {
  const wins = await getAllWebviewWindows();
  return wins.filter((w) => w.label.startsWith(FLEX_POPOUT_PREFIX));
}

async function outerLogicalPos(
  w: WebviewWindow,
): Promise<TauriOuterLogicalPos | null> {
  try {
    const pos = await w.outerPosition();
    const scale = await w.scaleFactor();
    return {
      label: w.label,
      x: pos.x / scale,
      y: pos.y / scale,
    };
  } catch {
    return null;
  }
}

/** Resolve the `flex-popout-*` handle for a popout browsing context (by position). */
export async function findFlexPopoutWebviewForDomWindow(
  domWin: Window,
): Promise<WebviewWindow | null> {
  const flex = await listFlexPopoutWindows();
  if (flex.length === 0) return null;

  const positions: TauriOuterLogicalPos[] = [];
  for (const w of flex) {
    const p = await outerLogicalPos(w);
    if (p) positions.push(p);
  }

  const label = matchFlexPopoutLabelByPosition(
    { screenX: domWin.screenX, screenY: domWin.screenY },
    positions,
  );
  if (!label) return null;
  return flex.find((w) => w.label === label) ?? null;
}

async function focusedFlexPopout(): Promise<WebviewWindow | null> {
  const flex = await listFlexPopoutWindows();
  for (const w of flex) {
    try {
      if (await w.isFocused()) return w;
    } catch {
      /* ignore */
    }
  }
  return null;
}

/**
 * Close the OS shell for one popout. Prefer matching `domWin` by position.
 * Fallbacks (never close-all): sole flex-popout; or focused when no domWin was
 * given (chrome click already focused that shell). If position miss with 2+
 * popouts, leave Tauri alone rather than kill a sibling.
 */
export async function closeFlexPopoutShell(
  domWin?: Window | null,
): Promise<void> {
  let tauriWin: WebviewWindow | null = null;

  if (domWin && !domWin.closed) {
    tauriWin = await findFlexPopoutWebviewForDomWindow(domWin);
    try {
      domWin.close();
    } catch {
      /* ignore */
    }
  }

  if (!tauriWin) {
    const flex = await listFlexPopoutWindows();
    if (flex.length === 1) {
      tauriWin = flex[0] ?? null;
    } else if (!domWin) {
      tauriWin = await focusedFlexPopout();
    }
  }

  if (!tauriWin) return;
  try {
    await tauriWin.close();
  } catch (err) {
    console.error('[closeFlexPopoutShell] WebviewWindow.close failed:', err);
  }
}
