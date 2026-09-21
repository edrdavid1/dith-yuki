import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import type { ReactNode } from 'react';
import EmptyState from '../EmptyState';
import { ShellProvider } from '../../app/shell/ShellContext';
import { openRecentByKind } from '../../shared/ipc/recent';
import type { RecentFileEntry } from '../../shared/ipc/recent';

function wrapper({ children }: { children: ReactNode }) {
  return <ShellProvider>{children}</ShellProvider>;
}

const imageEntry: RecentFileEntry = {
  path: '/tmp/photo.png',
  kind: 'image',
  display_name: 'photo.png',
  opened_at: '2026-08-13T00:00:00.000Z',
};

const projectEntry: RecentFileEntry = {
  path: '/tmp/proj.dyproj',
  kind: 'project',
  display_name: 'proj.dyproj',
  opened_at: '2026-08-13T00:00:00.000Z',
};

describe('EmptyState (Welcome)', () => {
  it('does not render the Recent section when the list is empty', () => {
    render(
      <EmptyState
        onNewProject={vi.fn()}
        onOpenImage={vi.fn()}
        onOpenProject={vi.fn()}
      />,
      { wrapper }
    );
    expect(screen.getByText('Open image')).toBeInTheDocument();
    expect(screen.getByText('Open project')).toBeInTheDocument();
    expect(screen.queryByTestId('welcome-recent')).not.toBeInTheDocument();
    expect(screen.queryByText('Recent')).not.toBeInTheDocument();
  });

  it('hides the Recent section when hideRecentList preference is on', () => {
    localStorage.setItem(
      'dither.shellPrefs',
      JSON.stringify({
        version: 2,
        hideRecentList: true,
        leftSidebar: { width: 332, collapsed: false },
        rightSidebar: { width: 332, collapsed: false },
        leftSplitRatio: 0.5,
        rightSplitRatio: 0.5,
        effectPanelRatio: 0.5,
        autoExtractPalettes: true,
        previewBackground: 'gray',
        welcomeBackground: 'artwork',
      })
    );
    render(<EmptyState recentEntries={[imageEntry]} />, { wrapper });
    expect(screen.queryByTestId('welcome-recent')).not.toBeInTheDocument();
    expect(screen.queryByText('Recent')).not.toBeInTheDocument();
  });

  it('maps an image recent row to openImageAt', () => {
    const openImageAt = vi.fn();
    const openProjectAt = vi.fn();
    render(
      <EmptyState
        recentEntries={[imageEntry]}
        onOpenRecent={(entry) =>
          openRecentByKind(entry, { openImageAt, openProjectAt })
        }
      />,
      { wrapper }
    );
    fireEvent.click(screen.getByText('photo.png'));
    expect(openImageAt).toHaveBeenCalledWith('/tmp/photo.png');
    expect(openProjectAt).not.toHaveBeenCalled();
  });

  it('maps a project recent row to openProjectAt', () => {
    const openImageAt = vi.fn();
    const openProjectAt = vi.fn();
    render(
      <EmptyState
        recentEntries={[projectEntry]}
        onOpenRecent={(entry) =>
          openRecentByKind(entry, { openImageAt, openProjectAt })
        }
      />,
      { wrapper }
    );
    fireEvent.click(screen.getByText('proj.dyproj'));
    expect(openProjectAt).toHaveBeenCalledWith('/tmp/proj.dyproj');
    expect(openImageAt).not.toHaveBeenCalled();
  });

  it('shows at most 6 recent entries', () => {
    const entries: RecentFileEntry[] = Array.from({ length: 8 }, (_, i) => ({
      path: `/tmp/file-${i}.png`,
      kind: 'image',
      display_name: `file-${i}.png`,
      opened_at: '2026-08-13T00:00:00.000Z',
    }));
    render(<EmptyState recentEntries={entries} />, { wrapper });
    expect(screen.getByText('file-0.png')).toBeInTheDocument();
    expect(screen.getByText('file-5.png')).toBeInTheDocument();
    expect(screen.queryByText('file-6.png')).not.toBeInTheDocument();
  });

  it('clears recent on double-click of the Recent heading', () => {
    const onClearRecent = vi.fn();
    render(
      <EmptyState recentEntries={[imageEntry]} onClearRecent={onClearRecent} />,
      { wrapper }
    );
    fireEvent.doubleClick(screen.getByText('Recent'));
    expect(onClearRecent).toHaveBeenCalledTimes(1);
  });
});
