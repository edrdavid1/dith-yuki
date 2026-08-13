import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import EmptyState from '../EmptyState';
import { openRecentByKind } from '../../shared/ipc/recent';
import type { RecentFileEntry } from '../../shared/ipc/recent';

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
      />
    );
    expect(screen.getByText('New Project')).toBeInTheDocument();
    expect(screen.getByText('Open Image…')).toBeInTheDocument();
    expect(screen.getByText('Open Project…')).toBeInTheDocument();
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
      />
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
      />
    );
    fireEvent.click(screen.getByText('proj.dyproj'));
    expect(openProjectAt).toHaveBeenCalledWith('/tmp/proj.dyproj');
    expect(openImageAt).not.toHaveBeenCalled();
  });
});
