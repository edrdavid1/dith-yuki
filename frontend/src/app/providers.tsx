import React, { useEffect, type ReactNode } from 'react';
import { Provider } from 'react-redux';
import { store } from './store';
import { startEngineEventBridge } from './listeners';
import { suppressBrowserChrome } from './suppressBrowserChrome';
import { ShellProvider } from './shell/ShellContext';
import { ShortcutsProvider } from '../features/shortcuts/ShortcutsContext';
import { useAppShortcuts } from '../features/shortcuts/useAppShortcuts';
import { LayoutProvider } from '../contexts/LayoutContext';
import { PatternsUiProvider } from '../features/patterns/PatternsUiContext';
import { WorkspaceProvider } from '../features/workspace/WorkspaceContext';
import { GamedevProvider } from '../features/workspace/GamedevContext';
import GamedevPanelVisibilitySync from '../features/workspace/GamedevPanelVisibilitySync';
import FlexPatchMissingOverlay from '../features/panels/FlexPatchMissingOverlay';
import { isFlexPatchApplied } from '../features/panels/flexlayoutPatchGate';
import { startFlexPopoutStyleSync } from '../features/panels/flexPopoutStyleSync';

function ShortcutEngine() {
  useAppShortcuts();
  return null;
}

/**
 * Root providers for the main App window.
 * RTK store + App Shell Context (layout prefs) + FlexLayout.
 */
export function Providers({ children }: { children: ReactNode }) {
  useEffect(() => {
    return startEngineEventBridge(store);
  }, []);

  useEffect(() => {
    return suppressBrowserChrome(document, import.meta.env.DEV);
  }, []);

  useEffect(() => {
    if (!isFlexPatchApplied()) {
      console.error(
        '[flex-dock] flexlayout-react __FLEX_PATCH__ missing — float disabled',
      );
    }
  }, []);

  // W5: HMR / theme head mutations → open flex-popout documents.
  useEffect(() => startFlexPopoutStyleSync(), []);

  return (
    <Provider store={store}>
      <ShellProvider>
        <WorkspaceProvider>
          <GamedevProvider>
            <PatternsUiProvider>
              <ShortcutsProvider>
                <ShortcutEngine />
                <LayoutProvider>
                  <GamedevPanelVisibilitySync />
                  {children}
                </LayoutProvider>
                <FlexPatchMissingOverlay />
              </ShortcutsProvider>
            </PatternsUiProvider>
          </GamedevProvider>
        </WorkspaceProvider>
      </ShellProvider>
    </Provider>
  );
}
