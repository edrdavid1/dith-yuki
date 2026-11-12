/**
 * W4: close every `flex-popout-*` OS shell before replacing FlexLayout models.
 */

import { getAllWebviewWindows } from '@tauri-apps/api/webviewWindow';
import { clearFlexPopoutRegistry } from './flexPopoutRegistry';

const FLEX_POPOUT_PREFIX = 'flex-popout-';

/** Best-effort close of all flex popout WebviewWindows. */
export async function closeAllFlexPopouts(): Promise<void> {
  let wins;
  try {
    wins = await getAllWebviewWindows();
  } catch (err) {
    console.error('[flex-dock] closeAllFlexPopouts: list windows failed', err);
    return;
  }

  const flex = wins.filter((w) => w.label.startsWith(FLEX_POPOUT_PREFIX));
  if (flex.length === 0) return;

  console.info('[flex-dock] closeAllFlexPopouts', flex.map((w) => w.label));
  await Promise.all(
    flex.map(async (w) => {
      try {
        await w.close();
      } catch (err) {
        console.error('[flex-dock] closeAllFlexPopouts: close failed', w.label, err);
      }
    }),
  );

  // Registry entries are stale after a mass close.
  clearFlexPopoutRegistry();
}
