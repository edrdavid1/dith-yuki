import {
  useState,
  useRef,
  useEffect,
  useCallback,
  useLayoutEffect,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import SimpleBar from 'simplebar-react';
import styles from './DropdownMenu.module.css';
import sliderStyles from '../../shared/ui/Slider.module.css';
import { bind } from '../../shared/ui/cn';
const cn = bind({ ...styles, ...sliderStyles });


export interface DropdownOption {
  value: string;
  label: string;
  disabled?: boolean;
}

interface DropdownMenuProps {
  label?: string;
  value: string;
  options: DropdownOption[];
  onSelect: (value: string) => void;
  disabled?: boolean;
  className?: string;
  /** Replaces the default label text inside the closed field (keeps field chrome). */
  selectedContent?: ReactNode;
  /** Custom menu row content; defaults to `option.label`. */
  renderOption?: (option: DropdownOption) => ReactNode;
}

export default function DropdownMenu({
  label,
  value,
  options,
  onSelect,
  disabled = false,
  className = '',
  selectedContent,
  renderOption,
}: DropdownMenuProps) {
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number; width: number } | null>(null);

  const selectedOption = options.find((option) => option.value === value);
  const displayLabel = selectedOption ? selectedOption.label : value;

  const toggleOpen = useCallback(() => {
    if (disabled) return;
    setIsOpen((open) => !open);
  }, [disabled]);

  const handleSelect = useCallback(
    (optionValue: string) => {
      if (disabled) return;
      setIsOpen(false);
      onSelect(optionValue);
    },
    [disabled, onSelect]
  );

  // Calculate menu position when opening via portal
  useLayoutEffect(() => {
    if (!isOpen || !containerRef.current) {
      setMenuPosition(null);
      return;
    }
    const rect = containerRef.current.getBoundingClientRect();
    setMenuPosition({
      top: rect.bottom,
      left: rect.left,
      width: rect.width,
    });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        // Also check if click is inside the portal-rendered menu
        const portalTarget = document.getElementById('overlay-portal');
        if (portalTarget?.contains(event.target as Node)) return;
        setIsOpen(false);
      }
    };
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
    };

    window.addEventListener('mousedown', handleClickOutside);
    window.addEventListener('keydown', handleEscape);
    return () => {
      window.removeEventListener('mousedown', handleClickOutside);
      window.removeEventListener('keydown', handleEscape);
    };
  }, [isOpen]);

  const renderMenu = () => {
    const menuContent = (
      <div
        className={cn("lp-dropdown-menu")}
        style={menuPosition ? {
          position: 'fixed',
          top: `${menuPosition.top}px`,
          left: `${menuPosition.left}px`,
          width: `${menuPosition.width}px`,
          right: 'auto',
        } : undefined}
      >
        <SimpleBar style={{ maxHeight: '220px' }}>
          <ul role="listbox" aria-label="Options list" style={{ margin: 0, padding: '2px 0', listStyle: 'none' }}>
            {options.map((option) => (
              <li
                key={option.value}
                className={cn(
                  'lp-dropdown-menu-item',
                  option.value === value && 'active',
                  option.disabled && 'disabled'
                )}
                role="option"
                aria-selected={option.value === value}
                onClick={() => !option.disabled && handleSelect(option.value)}
              >
                {renderOption ? renderOption(option) : option.label}
              </li>
            ))}
          </ul>
        </SimpleBar>
      </div>
    );

    // Try to render via portal to #overlay-portal
    const portalTarget = document.getElementById('overlay-portal');
    if (portalTarget && menuPosition) {
      return createPortal(menuContent, portalTarget);
    }

    // Graceful fallback: render inline if portal target not found
    return menuContent;
  };

  return (
    <div className={cn('lp-dropdown-wrap', className)} ref={containerRef}>
      {label ? <label className={cn("slider-label")}>{label}</label> : null}
      <div
        className={cn('lp-dropdown-field', disabled && 'disabled')}
        role="button"
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        tabIndex={disabled ? -1 : 0}
        onClick={toggleOpen}
        onKeyDown={(event) => {
          if (disabled) return;
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            toggleOpen();
          }
        }}
      >
        <span className={cn("lp-dropdown-field-text")}>
          {selectedContent ?? displayLabel}
        </span>
        <button
          type="button"
          className={cn("lp-dropdown-btn")}
          onClick={(event) => {
            event.stopPropagation();
            toggleOpen();
          }}
          disabled={disabled}
          aria-label="Open dropdown"
        />
      </div>
      {isOpen && renderMenu()}
    </div>
  );
}
