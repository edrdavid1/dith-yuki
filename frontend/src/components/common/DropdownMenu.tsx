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
  /** Consecutive options with the same group get a non-selectable header. */
  group?: string;
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
  const fieldRef = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number; width: number } | null>(null);

  const selectedOption = options.find((option) => option.value === value);
  const displayLabel = selectedOption ? selectedOption.label : value;

  const hostDocument = () =>
    containerRef.current?.ownerDocument ?? fieldRef.current?.ownerDocument ?? document;

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

  useLayoutEffect(() => {
    if (!isOpen || !fieldRef.current) {
      setMenuPosition(null);
      return;
    }
    const doc = hostDocument();
    const view = doc.defaultView ?? window;
    const pad = 6;

    const update = () => {
      const field = fieldRef.current;
      if (!field) return;
      const rect = field.getBoundingClientRect();
      const menuH = menuRef.current?.offsetHeight ?? 0;
      const menuW = Math.max(rect.width, menuRef.current?.offsetWidth ?? rect.width);
      const vw = view.innerWidth;
      const vh = view.innerHeight;

      let top = rect.bottom;
      let left = rect.left;
      if (menuH > 0 && top + menuH > vh - pad && rect.top - menuH >= pad) {
        top = rect.top - menuH;
      } else if (menuH > 0 && top + menuH > vh - pad) {
        top = Math.max(pad, vh - pad - menuH);
      }
      if (left + menuW > vw - pad) {
        left = Math.max(pad, vw - pad - menuW);
      }
      if (left < pad) left = pad;

      setMenuPosition((prev) => {
        if (
          prev &&
          prev.top === top &&
          prev.left === left &&
          prev.width === rect.width
        ) {
          return prev;
        }
        return { top, left, width: rect.width };
      });
    };

    update();
    // Second pass after the portaled menu mounts so we can flip using real height.
    const id = view.requestAnimationFrame(update);
    return () => view.cancelAnimationFrame(id);
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const doc = hostDocument();
    const view = doc.defaultView ?? window;
    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (containerRef.current?.contains(target)) return;
      if (menuRef.current?.contains(target)) return;
      setIsOpen(false);
    };
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
    };

    doc.addEventListener('mousedown', handleClickOutside);
    view.addEventListener('keydown', handleEscape);
    const handleScroll = (event: Event) => {
      const target = event.target;
      if (target instanceof Node && menuRef.current?.contains(target)) return;
      setIsOpen(false);
    };
    doc.addEventListener('scroll', handleScroll, true);
    view.addEventListener('wheel', handleScroll, { capture: true, passive: true });
    return () => {
      doc.removeEventListener('mousedown', handleClickOutside);
      view.removeEventListener('keydown', handleEscape);
      doc.removeEventListener('scroll', handleScroll, true);
      view.removeEventListener('wheel', handleScroll, true);
    };
  }, [isOpen]);

  const portalParent = hostDocument().body;
  const showMenu = isOpen && menuPosition != null && portalParent != null;

  return (
    <div className={cn('lp-dropdown-wrap', className)} ref={containerRef}>
      {label ? <label className={cn("slider-label")}>{label}</label> : null}
      <div
        ref={fieldRef}
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
      {showMenu &&
        createPortal(
          <div
            ref={menuRef}
            className={cn("lp-dropdown-menu")}
            style={{
              position: 'fixed',
              top: `${menuPosition.top}px`,
              left: `${menuPosition.left}px`,
              width: `${menuPosition.width}px`,
              right: 'auto',
            }}
          >
            <SimpleBar style={{ maxHeight: '220px' }}>
              <ul role="listbox" aria-label="Options list" style={{ margin: 0, padding: '2px 0', listStyle: 'none' }}>
                {options.map((option, index) => {
                  const prev = options[index - 1];
                  const showGroup = Boolean(option.group) && option.group !== prev?.group;
                  return (
                    <li key={option.value}>
                      {showGroup && (
                        <div className={cn('lp-dropdown-group')} role="presentation">
                          {option.group}
                        </div>
                      )}
                      <div
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
                      </div>
                    </li>
                  );
                })}
              </ul>
            </SimpleBar>
          </div>,
          portalParent
        )}
    </div>
  );
}
