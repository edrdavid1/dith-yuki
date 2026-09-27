/** Startup boot screen — shown only when first paint is slow. */

const BOOT_FADE_MS = 380;
/**
 * How long to wait before revealing the branded splash.
 * Fast cold starts finish under this and skip splash entirely (no flash).
 * 5s would leave the dock icon looking dead; ~400ms is the usual threshold.
 */
const BOOT_REVEAL_DELAY_MS = 400;
const BOOT_ASSET_URLS = ['/img/background-img-1.png', '/img/dith.png'];

let ready = false;
let splashVisible = false;
let revealTimer: ReturnType<typeof setTimeout> | null = null;

function isPanelWindow(): boolean {
  return document.documentElement.classList.contains('is-panel');
}

function clearMenubarStartupFocus(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement && active.closest('[role="menubar"]')) {
    active.blur();
  }
}

/** Drop the boot node immediately (panel popouts / fast path). */
export function removeBootScreen(): void {
  document.getElementById('boot-screen')?.remove();
}

/** Fade splash out after it was actually shown. */
function fadeOutBootScreen(): void {
  const boot = document.getElementById('boot-screen');
  if (!boot || boot.classList.contains('boot-screen-done')) return;
  boot.classList.add('boot-screen-done');
  clearMenubarStartupFocus();
  window.setTimeout(() => boot.remove(), BOOT_FADE_MS + 40);
}

/** Resolve once boot images are decoded (or failed). */
function waitForBootAssets(): Promise<void> {
  return Promise.all(
    BOOT_ASSET_URLS.map(
      (src) =>
        new Promise<void>((resolve) => {
          const img = new Image();
          img.onload = () => resolve();
          img.onerror = () => resolve();
          img.src = src;
        }),
    ),
  ).then(() => undefined);
}

/** Show the main window (starts hidden in tauri.conf). */
async function revealMainWindow(): Promise<void> {
  if (isPanelWindow()) return;
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    if (win.label !== 'main') return;
    // show() only — setFocus() parks keyboard focus on File in the titlebar.
    await win.show();
  } catch {
    // Browser / non-Tauri — nothing to show.
  }
}

async function showSplash(): Promise<void> {
  if (ready || splashVisible) return;
  splashVisible = true;
  const boot = document.getElementById('boot-screen');
  boot?.classList.remove('boot-deferred');
  await waitForBootAssets();
  if (ready) {
    // Became ready while assets loaded — skip splash after all.
    splashVisible = false;
    removeBootScreen();
    await revealMainWindow();
    return;
  }
  await revealMainWindow();
}

/**
 * Start the splash gate: preload in the background; only reveal splash if
 * `finishBoot` has not fired before {@link BOOT_REVEAL_DELAY_MS}.
 */
export function startBootGate(): void {
  if (isPanelWindow()) {
    removeBootScreen();
    return;
  }

  document.getElementById('boot-screen')?.classList.add('boot-deferred');
  // Warm decode; splash path awaits again if needed.
  void waitForBootAssets();

  revealTimer = setTimeout(() => {
    revealTimer = null;
    if (ready) return;
    void showSplash();
  }, BOOT_REVEAL_DELAY_MS);
}

/**
 * First real UI has painted. Fast path: never show splash. Slow path: fade it out.
 */
export function finishBoot(): void {
  if (isPanelWindow()) {
    removeBootScreen();
    return;
  }
  if (ready) return;
  ready = true;

  if (revealTimer != null) {
    clearTimeout(revealTimer);
    revealTimer = null;
  }

  clearMenubarStartupFocus();

  if (splashVisible) {
    fadeOutBootScreen();
  } else {
    removeBootScreen();
    void revealMainWindow();
  }
}

/** @deprecated Prefer {@link finishBoot}; kept for any stray callers. */
export function dismissBootScreen(): void {
  finishBoot();
}
