/** Boot lifecycle: keep the main window hidden until the first UI paint, then reveal. */

import { stopDockBounce } from '../shared/ipc/app';

let ready = false;

function isPanelWindow(): boolean {
  return document.documentElement.classList.contains('is-panel');
}

function clearMenubarStartupFocus(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement && active.closest('[role="menubar"]')) {
    active.blur();
  }
}

/** Drop any leftover boot node immediately if present. */
export function removeBootScreen(): void {
  document.getElementById('boot-screen')?.remove();
}

/** Show the main window (starts hidden in tauri.conf) and end Dock launch attention. */
async function revealMainWindow(): Promise<void> {
  if (isPanelWindow()) return;
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    if (win.label !== 'main') return;
    // show() only — setFocus() parks keyboard focus on File in the titlebar.
    await win.show();
    try {
      await stopDockBounce();
    } catch {
      // Non-Tauri / test environment.
    }
  } catch {
    // Browser / non-Tauri — nothing to show.
  }
}

/** Clear any leftover splash DOM; native side owns the slow-launch Dock marker. */
export function startBootGate(): void {
  removeBootScreen();
}

/**
 * First real UI has painted. Reveal the main window and stop launch attention.
 */
export function finishBoot(): void {
  if (isPanelWindow()) {
    return;
  }
  if (ready) return;
  ready = true;

  clearMenubarStartupFocus();
  void revealMainWindow();
}

/** @deprecated Prefer {@link finishBoot}; kept for any stray callers. */
export function dismissBootScreen(): void {
  finishBoot();
}
