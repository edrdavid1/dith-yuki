import React, { useEffect, type ReactNode } from 'react';
import { Provider } from 'react-redux';
import { store } from './store';
import { startEngineEventBridge } from './listeners';
import { suppressBrowserChrome } from './suppressBrowserChrome';
import { ShellProvider } from './shell/ShellContext';
import { ShortcutsProvider } from '../features/shortcuts/ShortcutsContext';
import { useAppShortcuts } from '../features/shortcuts/useAppShortcuts';
import { LayoutProvider } from '../contexts/LayoutContext';

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
    return suppressBrowserChrome();
  }, []);

  return (
    <Provider store={store}>
      <ShellProvider>
        <ShortcutsProvider>
          <ShortcutEngine />
          <LayoutProvider>
            {children}
          </LayoutProvider>
        </ShortcutsProvider>
      </ShellProvider>
    </Provider>
  );
}
