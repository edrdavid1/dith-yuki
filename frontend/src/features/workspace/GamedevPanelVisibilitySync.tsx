/**
 * Shows / hides the Gamedev Settings FlexLayout panel with the Gamedev workspace.
 * On enter, injects into the less-crowded sidebar (usually the single-panel side).
 * While staying in Gamedev, does not fight user moves — only re-injects if missing.
 */

import { useEffect, useRef } from 'react';
import { useLayoutContext } from '../../contexts/LayoutContext';
import { listFlexComponents } from '../../contexts/layoutModelOps';
import { preferredSideForGamedevPanel } from './gamedevPanelPlacement';
import { useWorkspaceOptional } from './WorkspaceContext';

export default function GamedevPanelVisibilitySync() {
  const workspace = useWorkspaceOptional();
  const {
    layoutEpoch,
    left,
    right,
    ensureOptionalPanel,
    removeOptionalPanel,
  } = useLayoutContext();

  const show = Boolean(workspace?.visibility.showGamedevStrip);
  const ready =
    Boolean(workspace?.ready) && !left.isLoading && !right.isLoading;
  const wasShowing = useRef(false);

  useEffect(() => {
    if (!ready) return;

    if (!show) {
      removeOptionalPanel('gamedev');
      wasShowing.current = false;
      return;
    }

    const side = preferredSideForGamedevPanel(left.model, right.model);
    const present =
      listFlexComponents(left.model).includes('gamedev') ||
      listFlexComponents(right.model).includes('gamedev');

    if (!wasShowing.current) {
      // Entering Gamedev: place on the preferred (less crowded) side.
      if (present) removeOptionalPanel('gamedev');
      ensureOptionalPanel('gamedev', side);
      wasShowing.current = true;
      return;
    }

    // Still in Gamedev — restore only if the panel vanished (e.g. layout reset).
    if (!present) ensureOptionalPanel('gamedev', side);
  }, [
    show,
    ready,
    layoutEpoch,
    left.model,
    right.model,
    ensureOptionalPanel,
    removeOptionalPanel,
  ]);

  return null;
}
