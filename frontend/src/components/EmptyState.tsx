import styles from '../shared/ui/EmptyState.module.css';
import { bind } from '../shared/ui/cn';
import { formatRelativeTime } from '../shared/relativeTime';
import type { RecentFileEntry } from '../shared/ipc/recent';
import Icon from '../icons/iconRegistry';

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
}

function EmptyState({
  className,
  fill = false,
  recentEntries = [],
  onOpenImage,
  onOpenProject,
  onOpenRecent,
}: EmptyStateProps) {
  const visibleRecent = recentEntries.slice(0, WELCOME_RECENT_LIMIT);

  return (
    <div className={cn('empty-state', fill && 'empty-state-fill', className)}>
      <div className={cn('welcome-brand')}>
        <img
          className={cn('welcome-hero')}
          src="/img/dith.png"
          alt="Dither Yuki"
        />
      </div>

      <div className={cn('welcome-actions')}>
        <button type="button" className={cn('welcome-action')} onClick={onOpenImage}>
          Open image
        </button>
        <button type="button" className={cn('welcome-action')} onClick={onOpenProject}>
          Open project
        </button>
      </div>

      {visibleRecent.length > 0 && (
        <section className={cn('welcome-recent')} data-testid="welcome-recent" aria-label="Recent files">
          <h2 className={cn('welcome-recent-heading')}>Recent</h2>
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
  );
}

export default EmptyState;
