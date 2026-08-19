import React, { useEffect, type ReactNode } from 'react';
import { Provider } from 'react-redux';
import { store } from './store';
import { startEngineEventBridge } from './listeners';
import { ShellProvider } from './shell/ShellContext';
import { ShortcutsProvider } from '../features/shortcuts/ShortcutsContext';
import { useAppShortcuts } from '../features/shortcuts/useAppShortcuts';

function ShortcutEngine() {
  useAppShortcuts();
  return null;
}

/**
 * Root providers for main App and floating PanelWindow.
 * RTK store + App Shell Context (layout prefs, not persisted).
 */
export function Providers({ children }: { children: ReactNode }) {
  useEffect(() => {
    return startEngineEventBridge(store);
  }, []);

  return (
    <Provider store={store}>
      <ShellProvider>
        <ShortcutsProvider>
          <ShortcutEngine />
          {children}
        </ShortcutsProvider>
      </ShellProvider>
    </Provider>
  );
}
