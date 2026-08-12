// src/components/WindowShell.tsx
import React from 'react';
import { AppTitlebar } from './AppTitlebar';

interface WindowShellProps {
  children: React.ReactNode;
  /** Optional title displayed in the titlebar (for panel windows) */
  title?: string;
  /** Optional: additional titlebar content (e.g., menu bar) */
  titlebarContent?: React.ReactNode;
}

export function WindowShell({ children, title, titlebarContent }: WindowShellProps) {
  return (
    <div className="window-shell">
      <AppTitlebar title={title}>
        {titlebarContent}
      </AppTitlebar>
      <div className="content-area">
        {children}
      </div>
      <div id="overlay-portal" />
    </div>
  );
}
