import { useEffect, useRef, useState, useCallback } from 'react';
import type { EffectType } from '../types/effects';
import styles from '../features/effects/EffectChooserDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
const cn = bind(styles);

interface EffectChooserDialogProps {
  isOpen: boolean;
  onSelect: (effectType: EffectType) => void;
  onClose: () => void;
}

interface EffectOption {
  type: EffectType;
  icon: string;
  label: string;
}

const EFFECT_OPTIONS: EffectOption[] = [
  { type: 'Dithering', icon: '🎨', label: 'Dithering' },
  { type: 'Glitching', icon: '⚡', label: 'Glitching' },
  { type: 'Curves', icon: '📈', label: 'Curves' },
  { type: 'RGBChannels', icon: '🔴', label: 'RGB Channels' },
  { type: 'Glow', icon: '✨', label: 'Glow' },
  { type: 'CRT', icon: '📺', label: 'CRT' },
  { type: 'Adjust', icon: '🎚️', label: 'Adjust' },
];

function EffectChooserDialog({ isOpen, onSelect, onClose }: EffectChooserDialogProps) {
  const [focusedIndex, setFocusedIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // Reset focus when dialog opens
  useEffect(() => {
    if (isOpen) {
      setFocusedIndex(0);
      // Focus the first item after render
      requestAnimationFrame(() => {
        itemRefs.current[0]?.focus();
      });
    }
  }, [isOpen]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          setFocusedIndex((prev) => {
            const next = Math.min(prev + 1, EFFECT_OPTIONS.length - 1);
            itemRefs.current[next]?.focus();
            return next;
          });
          break;
        case 'ArrowUp':
          e.preventDefault();
          setFocusedIndex((prev) => {
            const next = Math.max(prev - 1, 0);
            itemRefs.current[next]?.focus();
            return next;
          });
          break;
        case 'Enter':
          e.preventDefault();
          onSelect(EFFECT_OPTIONS[focusedIndex].type);
          break;
        case 'Escape':
          e.preventDefault();
          onClose();
          break;
      }
    },
    [focusedIndex, onSelect, onClose]
  );

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) {
        onClose();
      }
    },
    [onClose]
  );

  if (!isOpen) return null;

  return (
    <div
      className={cn("effect-chooser-overlay")}
      onClick={handleOverlayClick}
      data-testid="effect-chooser-overlay"
    >
      <div
        className={cn("effect-chooser-dialog")}
        role="dialog"
        aria-modal="true"
        aria-label="Effect"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar title="Effect" onClose={onClose} />

        {/* Effect list */}
        <div className={cn("effect-chooser-body")} ref={listRef} role="listbox" aria-label="Effect types">
          {EFFECT_OPTIONS.map((option, index) => (
            <button
              key={option.type}
              ref={(el) => { itemRefs.current[index] = el; }}
              className={cn("effect-chooser-item", focusedIndex === index && "effect-chooser-item-focused")}
              role="option"
              aria-selected={focusedIndex === index}
              onClick={() => onSelect(option.type)}
              onFocus={() => setFocusedIndex(index)}
              tabIndex={focusedIndex === index ? 0 : -1}
              type="button"
            >
              <span className={cn("effect-chooser-item-icon")}>{option.icon}</span>
              <span className={cn("effect-chooser-item-label")}>{option.label}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

export default EffectChooserDialog;
