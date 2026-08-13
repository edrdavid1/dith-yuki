import styles from '../shared/ui/EmptyState.module.css';
import { bind } from '../shared/ui/cn';
import { formatRelativeTime } from '../shared/relativeTime';
import type { RecentFileEntry } from '../shared/ipc/recent';

const cn = bind(styles);

export interface EmptyStateProps {
  className?: string;
  fill?: boolean;
  recentEntries?: RecentFileEntry[];
  onNewProject?: () => void;
  onOpenImage?: () => void;
  onOpenProject?: () => void;
  onOpenRecent?: (entry: RecentFileEntry) => void;
}

function EmptyState({
  className,
  fill = false,
  recentEntries = [],
  onNewProject,
  onOpenImage,
  onOpenProject,
  onOpenRecent,
}: EmptyStateProps) {
  return (
    <div className={cn('empty-state', fill && 'empty-state-fill', className)}>
      <div className={cn('welcome-brand')}>
        <span className={cn('welcome-logo')} aria-hidden>
          ▦
        </span>
        <h1 className={cn('welcome-title')}>Dither</h1>
      </div>

      <div className={cn('welcome-actions')}>
        <button type="button" className={cn('welcome-action')} onClick={onNewProject}>
          New Project
        </button>
        <button type="button" className={cn('welcome-action')} onClick={onOpenImage}>
          Open Image…
        </button>
        <button type="button" className={cn('welcome-action')} onClick={onOpenProject}>
          Open Project…
        </button>
      </div>

      {recentEntries.length > 0 && (
        <section className={cn('welcome-recent')} data-testid="welcome-recent" aria-label="Recent files">
          <h2 className={cn('welcome-recent-heading')}>Recent</h2>
          <ul className={cn('welcome-recent-list')}>
            {recentEntries.map((entry) => (
              <li key={entry.path}>
                <button
                  type="button"
                  className={cn('welcome-recent-row')}
                  onClick={() => onOpenRecent?.(entry)}
                >
                  <span className={cn('welcome-recent-icon')} aria-hidden>
                    {entry.kind === 'image' ? '🖼' : '📦'}
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
  );
}

export default EmptyState;
