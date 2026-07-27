import { useState, useCallback, useRef, useEffect } from 'react';

export interface ZoomControlsProps {
  zoom: number;
  onZoomChange: (zoom: number) => void;
  onFitToView: () => void;
}

const ZOOM_PRESETS = [0.25, 0.5, 1.0, 2.0, 4.0] as const;
const MIN_ZOOM_PERCENT = 1;
const MAX_ZOOM_PERCENT = 6400;

/**
 * ZoomControls displays the current zoom level as a percentage,
 * provides preset buttons (Fit, 25%, 50%, 100%, 200%, 400%),
 * and an editable text input for exact zoom values (1%–6400%).
 */
export default function ZoomControls({ zoom, onZoomChange, onFitToView }: ZoomControlsProps) {
  const displayPercent = Math.round(zoom * 100);
  const [isEditing, setIsEditing] = useState(false);
  const [inputValue, setInputValue] = useState(String(displayPercent));
  const inputRef = useRef<HTMLInputElement>(null);

  // Sync input value when zoom changes externally and not editing
  useEffect(() => {
    if (!isEditing) {
      setInputValue(String(displayPercent));
    }
  }, [displayPercent, isEditing]);

  const commitInput = useCallback(() => {
    setIsEditing(false);
    const parsed = parseInt(inputValue, 10);
    if (isNaN(parsed)) return;
    const clamped = Math.max(MIN_ZOOM_PERCENT, Math.min(MAX_ZOOM_PERCENT, parsed));
    onZoomChange(clamped / 100);
  }, [inputValue, onZoomChange]);

  const handleInputKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter') {
        commitInput();
        inputRef.current?.blur();
      } else if (e.key === 'Escape') {
        setIsEditing(false);
        setInputValue(String(displayPercent));
        inputRef.current?.blur();
      }
    },
    [commitInput, displayPercent],
  );

  const handleFocus = useCallback(() => {
    setIsEditing(true);
    setInputValue(String(displayPercent));
  }, [displayPercent]);

  const handleBlur = useCallback(() => {
    commitInput();
  }, [commitInput]);

  return (
    <div className="zoom-controls">
      <button
        className="zoom-preset-btn"
        onClick={onFitToView}
        title="Fit to view"
      >
        Fit
      </button>

      {ZOOM_PRESETS.map((preset) => (
        <button
          key={preset}
          className={`zoom-preset-btn${zoom === preset ? ' zoom-preset-active' : ''}`}
          onClick={() => onZoomChange(preset)}
          title={`Zoom to ${Math.round(preset * 100)}%`}
        >
          {Math.round(preset * 100)}%
        </button>
      ))}

      <div className="zoom-input-wrapper">
        <input
          ref={inputRef}
          className="zoom-input"
          type="text"
          inputMode="numeric"
          value={isEditing ? inputValue : `${displayPercent}`}
          onChange={(e) => setInputValue(e.target.value)}
          onFocus={handleFocus}
          onBlur={handleBlur}
          onKeyDown={handleInputKeyDown}
          aria-label="Zoom percentage"
          title="Type exact zoom percentage (1–6400)"
        />
        {!isEditing && <span className="zoom-input-suffix">%</span>}
      </div>
    </div>
  );
}
