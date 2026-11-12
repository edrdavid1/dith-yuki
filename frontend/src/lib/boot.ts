/** Boot lifecycle: splash is feedback only; main shows as soon as UI can paint. */

import { invoke } from '@tauri-apps/api/core';

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

/** Drop any leftover in-page boot node immediately if present. */
export function removeBootScreen(): void {
  document.getElementById('boot-screen')?.remove();
}

/** Clear any leftover in-page splash DOM; OS splash is owned by Rust. */
export function startBootGate(): void {
  removeBootScreen();
}

/**
 * First real UI has painted. Show main immediately (same as pre-splash), then
 * ask Rust to tear down the splash window. No minimum splash hold.
 */
export async function finishBoot(): Promise<void> {
  if (isPanelWindow()) {
    return;
  }
  if (ready) return;
  ready = true;

  clearMenubarStartupFocus();

  // Two frames: first UI frame is painted; don't reveal on a blank page.
  await new Promise<void>((r) =>
    requestAnimationFrame(() => requestAnimationFrame(() => r())),
  );

  try {
    // Show from JS first so splash IPC/destroy cannot delay first paint.
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    if (win.label === 'main') {
      await win.show();
    }
  } catch {
    // Browser / non-Tauri — nothing to show.
  }

  try {
    // Fire-and-forget teardown; main is already visible.
    void invoke('boot_finished');
  } catch (e) {
    console.error('[boot] boot_finished failed', e);
  }
}

/** Optional stage text on the splash (`window.setStage` via Rust eval). */
export function bootStage(text: string): void {
  invoke('boot_stage', { text }).catch((e) =>
    console.debug('[boot] boot_stage failed', e),
  );
}

/** @deprecated Prefer {@link finishBoot}; kept for any stray callers. */
export function dismissBootScreen(): void {
  void finishBoot();
}
