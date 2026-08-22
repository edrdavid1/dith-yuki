import { useAppDispatch, useAppSelector } from '../../app/hooks';
import { activateTab } from '../../app/slices/tabsSlice';
import type { OpenDocumentTab } from '../../shared/ipc/document';
import styles from './DocumentTabBar.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

/**
 * Chrome / VS Code tab strip. Close (×) goes through the window UnsavedGuard
 * (parent) so quit and tab-close share one dialog pipeline.
 */
export default function DocumentTabBar({
  onNewProject,
  onCloseTab,
}: {
  onNewProject: () => void;
  onCloseTab: (tab: OpenDocumentTab) => void;
}) {
  const dispatch = useAppDispatch();
  const { tabs, activeId } = useAppSelector((s) => s.tabs);

  return (
    <div className={cn('tab-bar')} data-tauri-drag-region>
      {tabs.map((tab) => {
        const active = tab.id === activeId;
        return (
          <button
            key={tab.id}
            type="button"
            className={cn('tab', active && 'tab-active')}
            data-tauri-drag-region="false"
            aria-current={active ? 'page' : undefined}
            onClick={() => {
              if (!active) void dispatch(activateTab(tab.id));
            }}
          >
            <span className={cn('tab-title')}>
              {tab.dirty ? '* ' : ''}
              {tab.title}
            </span>
            <span
              className={cn('tab-close')}
              role="button"
              tabIndex={0}
              aria-label={`Close ${tab.title}`}
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
        aria-label="New project"
        onClick={onNewProject}
      >
        +
      </button>
      <div className={cn('tab-drag-rest')} data-tauri-drag-region />
    </div>
  );
}
