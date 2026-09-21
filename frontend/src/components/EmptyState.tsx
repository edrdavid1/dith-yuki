import { useCallback, useEffect, useRef, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import styles from '../shared/ui/EmptyState.module.css';
import { bind } from '../shared/ui/cn';
import { formatRelativeTime } from '../shared/relativeTime';
import type { RecentFileEntry } from '../shared/ipc/recent';
import Icon from '../icons/iconRegistry';
import { useShell } from '../app/shell/ShellContext';
import { welcomeBackgroundStyle } from '../features/preview/welcomeBackground';
import { classifyDroppedPath } from '../shared/openDroppedPaths';

const cn = bind(styles);

const WELCOME_RECENT_LIMIT = 6;

export interface EmptyStateProps {
  className?: string;
  fill?: boolean;
  recentEntries?: RecentFileEntry[];
  onNewProject?: () => void;
  onOpenImage?: () => void;
  onOpenProject?: () => void;
  onOpenRecent?: (entry: RecentFileEntry) => void;
  onClearRecent?: () => void;
  onOpenDroppedPaths?: (paths: string[]) => void;
}

function physicalPointOverElement(
  el: HTMLElement,
  physical: { x: number; y: number },
): boolean {
  const dpr = window.devicePixelRatio || 1;
  const x = physical.x / dpr;
  const y = physical.y / dpr;
  const r = el.getBoundingClientRect();
  return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
}

function EmptyState({
  className,
  fill = false,
  recentEntries = [],
  onOpenImage,
  onOpenProject,
  onOpenRecent,
  onClearRecent,
  onOpenDroppedPaths,
}: EmptyStateProps) {
  const { welcomeBackground, hideRecentList } = useShell();
  const visibleRecent = hideRecentList ? [] : recentEntries.slice(0, WELCOME_RECENT_LIMIT);
  const rootRef = useRef<HTMLDivElement>(null);
  const [dropActive, setDropActive] = useState(false);

  const acceptPaths = useCallback(
    (paths: string[]) => paths.filter((p) => classifyDroppedPath(p) !== null),
    [],
  );

  useEffect(() => {
    if (!onOpenDroppedPaths) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const root = rootRef.current;
        if (!root) return;
        const payload = event.payload;

        if (payload.type === 'leave') {
          setDropActive(false);
          return;
        }

        const over = physicalPointOverElement(root, payload.position);
        if (payload.type === 'enter') {
          setDropActive(over && acceptPaths(payload.paths).length > 0);
          return;
        }
        if (payload.type === 'over') {
          setDropActive(over);
          return;
        }

        if (payload.type === 'drop') {
          setDropActive(false);
          if (!over) return;
          const usable = acceptPaths(payload.paths);
          if (usable.length > 0) onOpenDroppedPaths(usable);
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        /* web / non-tauri */
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [acceptPaths, onOpenDroppedPaths]);

  return (
    <div
      ref={rootRef}
      className={cn(
        'empty-state',
        fill && 'empty-state-fill',
        dropActive && 'empty-state-drop-active',
        className,
      )}
      style={welcomeBackgroundStyle(welcomeBackground)}
      data-testid="welcome-drop-zone"
      aria-label="Drop an image or project to open"
    >
      <div className={cn('welcome-content')}>
        <div className={cn('welcome-actions')}>
          <button type="button" className={cn('welcome-action')} onClick={onOpenImage}>
            Open image
          </button>
          <button type="button" className={cn('welcome-action')} onClick={onOpenProject}>
            Open project
          </button>
        </div>

        {/* Fixed-height slot = full Recent (6 rows) so actions don't jump when list clears/hides. */}
        <div className={cn('welcome-recent-slot')}>
          {visibleRecent.length > 0 && (
            <section className={cn('welcome-recent')} data-testid="welcome-recent" aria-label="Recent files">
              <h2
                className={cn('welcome-recent-heading')}
                title="Double-click to clear"
                onDoubleClick={() => onClearRecent?.()}
              >
                Recent
              </h2>
              <ul className={cn('welcome-recent-list')}>
                {visibleRecent.map((entry) => (
                  <li key={entry.path}>
                    <button
                      type="button"
                      className={cn('welcome-recent-row')}
                      onClick={() => onOpenRecent?.(entry)}
                    >
                      <span className={cn('welcome-recent-icon')} aria-hidden>
                        <Icon
                          name={entry.kind === 'image' ? 'row-img' : 'save'}
                          width={24}
                          height={24}
                        />
                      </span>
                      <span className={cn('welcome-recent-text')}>
                        <span className={cn('welcome-recent-name')}>{entry.display_name}</span>
                        <span className={cn('welcome-recent-path')} title={entry.path}>
                          {entry.path}
                        </span>
                      </span>
                      <span className={cn('welcome-recent-time')}>
                        {formatRelativeTime(entry.opened_at)}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}

export default EmptyState;
