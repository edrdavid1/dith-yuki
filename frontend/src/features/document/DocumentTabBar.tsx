import { useAppDispatch, useAppSelector } from '../../app/hooks';
import { activateTab } from '../../app/slices/tabsSlice';
import type { OpenDocumentTab } from '../../shared/ipc/document';
import { projectBasename } from '../../shared/unsavedGuard';
import styles from './DocumentTabBar.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

/**
 * Chrome / VS Code tab strip. Close (×) goes through the window UnsavedGuard
 * (parent) so quit and tab-close share one dialog pipeline.
 */
export default function DocumentTabBar({
  onOpenFile,
  onCloseTab,
}: {
  onOpenFile: () => void;
  onCloseTab: (tab: OpenDocumentTab) => void;
}) {
  const dispatch = useAppDispatch();
  const { tabs, activeId } = useAppSelector((s) => s.tabs);

  return (
    <div className={cn('tab-bar')} data-tauri-drag-region>
      {tabs.map((tab) => {
        const active = tab.id === activeId;
        // Display the actual file name from path, fallback to title for unsaved documents
        const displayName = tab.path ? projectBasename(tab.path) : tab.title;
        // Full path for tooltip
        const fullPath = tab.path || tab.title;
        return (
          <button
            key={tab.id}
            type="button"
            className={cn('tab', active && 'tab-active')}
            data-tauri-drag-region="false"
            aria-current={active ? 'page' : undefined}
            title={fullPath}
            onClick={() => {
              if (!active) void dispatch(activateTab(tab.id));
            }}
          >
            <span className={cn('tab-title')}>
              {tab.dirty ? '* ' : ''}
              {displayName}
            </span>
            <span
              className={cn('tab-close')}
              role="button"
              tabIndex={0}
              aria-label={`Close ${displayName}`}
              onClick={(e) => {
                e.stopPropagation();
                onCloseTab(tab);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  e.stopPropagation();
                  onCloseTab(tab);
                }
              }}
            >
              ×
            </span>
          </button>
        );
      })}
      <button
        type="button"
        className={cn('tab-new')}
        data-tauri-drag-region="false"
        aria-label="Open file"
        onClick={onOpenFile}
      >
        +
      </button>
      <div className={cn('tab-drag-rest')} data-tauri-drag-region />
    </div>
  );
}
