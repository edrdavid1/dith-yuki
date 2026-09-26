import { useState, useCallback, useEffect, useRef } from 'react';
import styles from '../features/document/MenuBar.module.css';
import { bind } from '../shared/ui/cn';
import type { RecentFileEntry } from '../shared/ipc/recent';
import { formatChords } from '../features/shortcuts/bindings';
import { useShortcutBindings } from '../features/shortcuts/ShortcutsContext';
const cn = bind(styles);

interface MenuBarProps {
  hasDocument: boolean;
  canUndo?: boolean;
  canRedo?: boolean;
  recentEntries?: RecentFileEntry[];
  onNewProject?: () => void;
  onOpenImage: () => void;
  onSaveImage: () => void;
  onExportAscii?: () => void;
  onCopyAsciiText?: () => void;
  onCopyAsciiAnsi?: () => void;
  onOpenProject: () => void;
  onOpenRecent?: (entry: RecentFileEntry) => void;
  onSaveProject: () => void;
  onSaveProjectAs: () => void;
  onShareProjectCopy?: () => void;
  onExportPattern: () => void;
  onImportPattern: () => void;
  onApplyCrossStitch?: () => void;
  onOpenPatterns: () => void;
  onOpenPreferences: () => void;
  onOpenHelp: () => void;
  onUndo?: () => void;
  onRedo?: () => void;
}

type MenuId = 'file' | 'edit' | 'patterns' | 'preferences' | 'help';

interface MenuItem {
  id: MenuId;
  label: string;
}

const MENU_ITEMS: MenuItem[] = [
  { id: 'file', label: 'File' },
  { id: 'edit', label: 'Edit' },
  { id: 'patterns', label: 'Patterns' },
  { id: 'preferences', label: 'Preferences' },
  { id: 'help', label: 'Help' },
];

/** Top-level items that open a window directly (no dropdown). */
const DIRECT_OPEN_MENUS: ReadonlySet<MenuId> = new Set(['patterns', 'preferences', 'help']);

function MenuBar({
  hasDocument,
  canUndo = false,
  canRedo = false,
  recentEntries = [],
  onNewProject,
  onOpenImage,
  onSaveImage,
  onExportAscii,
  onCopyAsciiText,
  onCopyAsciiAnsi,
  onOpenProject,
  onOpenRecent,
  onSaveProject,
  onSaveProjectAs,
  onShareProjectCopy,
  onExportPattern,
  onImportPattern,
  onApplyCrossStitch,
  onOpenPatterns,
  onOpenPreferences,
  onOpenHelp,
  onUndo,
  onRedo,
}: MenuBarProps) {
  const [openMenu, setOpenMenu] = useState<MenuId | null>(null);
  const menuBarRef = useRef<HTMLDivElement>(null);
  const shortcuts = useShortcutBindings();

  // Close dropdown on Escape or click-outside
  useEffect(() => {
    if (!openMenu) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpenMenu(null);
      }
    };

    const handleClickOutside = (e: MouseEvent) => {
      if (menuBarRef.current && !menuBarRef.current.contains(e.target as Node)) {
        setOpenMenu(null);
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [openMenu]);

  const handleMenuClick = useCallback((id: MenuId) => {
    if (id === 'patterns') {
      onOpenPatterns();
      setOpenMenu(null);
      return;
    }
    if (id === 'preferences') {
      onOpenPreferences();
      setOpenMenu(null);
      return;
    }
    if (id === 'help') {
      onOpenHelp();
      setOpenMenu(null);
      return;
    }
    setOpenMenu(prev => (prev === id ? null : id));
  }, [onOpenPatterns, onOpenPreferences, onOpenHelp]);

  const handleMenuHover = useCallback((id: MenuId) => {
    // Only switch on hover if a dropdown is already open
    if (openMenu !== null) {
      if (DIRECT_OPEN_MENUS.has(id)) {
        setOpenMenu(null);
        return;
      }
      setOpenMenu(id);
    }
  }, [openMenu]);

  const handleAction = useCallback((action: () => void) => {
    action();
    setOpenMenu(null);
  }, []);

  const renderDropdown = (id: MenuId) => {
    if (openMenu !== id) return null;

    switch (id) {
      case 'file':
        return (
          <div className={cn("menubar-dropdown")} role="menu">
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => handleAction(onNewProject ?? (() => {}))}
            >
              <span>New Project…</span>
              <span className={cn('menubar-shortcut')}>{formatChords(shortcuts.newProject)}</span>
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => handleAction(onOpenImage)}
            >
              <span>Open Image</span>
              <span className={cn('menubar-shortcut')}>{formatChords(shortcuts.openImage)}</span>
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => handleAction(onOpenProject)}
            >
              <span>Open Project…</span>
              <span className={cn('menubar-shortcut')}>{formatChords(shortcuts.openProject)}</span>
            </button>
            {recentEntries.length > 0 && (
              <div className={cn('menubar-submenu-wrap')}>
                <button
                  className={cn('menubar-dropdown-item', 'menubar-submenu-trigger')}
                  role="menuitem"
                  aria-haspopup="true"
                  type="button"
                >
                  Open Recent
                </button>
                <div className={cn('menubar-submenu')} role="menu" aria-label="Open Recent">
                  {recentEntries.map((entry) => (
                    <button
                      key={entry.path}
                      className={cn('menubar-dropdown-item')}
                      role="menuitem"
                      type="button"
                      onClick={() => handleAction(() => onOpenRecent?.(entry))}
                    >
                      {entry.display_name}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => handleAction(onSaveProject)}
              disabled={!hasDocument}
            >
              <span>Save Project</span>
              <span className={cn('menubar-shortcut')}>{formatChords(shortcuts.saveProject)}</span>
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => handleAction(onSaveProjectAs)}
              disabled={!hasDocument}
            >
              <span>Save Project As…</span>
              <span className={cn('menubar-shortcut')}>{formatChords(shortcuts.saveProjectAs)}</span>
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => onShareProjectCopy && handleAction(onShareProjectCopy)}
              disabled={!hasDocument || !onShareProjectCopy}
            >
              <span>Share Copy…</span>
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => handleAction(onSaveImage)}
              disabled={!hasDocument}
            >
              Save/Export
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => onExportAscii && handleAction(onExportAscii)}
              disabled={!hasDocument || !onExportAscii}
            >
              Export ASCII…
            </button>
            <button
              className={cn('menubar-dropdown-item')}
              role="menuitem"
              onClick={() => onApplyCrossStitch && handleAction(onApplyCrossStitch)}
              disabled={!hasDocument || !onApplyCrossStitch}
            >
              Apply Cross Stitch
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => handleAction(onExportPattern)}
              disabled={!hasDocument}
            >
              Export Pattern…
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => handleAction(onImportPattern)}
              disabled={!hasDocument}
            >
              Import Pattern…
            </button>
          </div>
        );
      case 'edit': {
        return (
          <div className={cn("menubar-dropdown")} role="menu">
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              disabled={!canUndo}
              onClick={() => canUndo && onUndo && handleAction(onUndo)}
            >
              <span>Undo</span>
              <span className={cn('menubar-shortcut')}>{formatChords(shortcuts.undo)}</span>
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              disabled={!canRedo}
              onClick={() => canRedo && onRedo && handleAction(onRedo)}
            >
              <span>Redo</span>
              <span className={cn('menubar-shortcut')}>{formatChords(shortcuts.redo)}</span>
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => onCopyAsciiText && handleAction(onCopyAsciiText)}
              disabled={!hasDocument || !onCopyAsciiText}
            >
              Copy ASCII Text
            </button>
            <button
              className={cn("menubar-dropdown-item")}
              role="menuitem"
              onClick={() => onCopyAsciiAnsi && handleAction(onCopyAsciiAnsi)}
              disabled={!hasDocument || !onCopyAsciiAnsi}
            >
              Copy ASCII ANSI
            </button>
          </div>
        );
      }
      default:
        return null;
    }
  };

  return (
    <div className={cn("menubar")} ref={menuBarRef} role="menubar">
      {MENU_ITEMS.map((item) => (
        <div className={cn("menubar-item")} key={item.id}>
          <button
            className={cn('toolbar-btn', openMenu === item.id && 'toolbar-btn-active')}
            onClick={() => handleMenuClick(item.id)}
            onMouseEnter={() => handleMenuHover(item.id)}
            role="menuitem"
            aria-haspopup={!DIRECT_OPEN_MENUS.has(item.id)}
            aria-expanded={openMenu === item.id}
          >
            {item.label}
          </button>
          {renderDropdown(item.id)}
        </div>
      ))}
    </div>
  );
}

export default MenuBar;
