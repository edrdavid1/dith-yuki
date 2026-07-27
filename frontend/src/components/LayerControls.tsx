import { useState, useCallback, useRef } from 'react';
import type { LayerNodeDto, LayerPropsPatch } from './LayerPanel';

// ─── Constants ────────────────────────────────────────────────────────────────

const BLEND_MODES = [
  'Normal',
  'Multiply',
  'Screen',
  'Overlay',
  'Darken',
  'Lighten',
  'ColorDodge',
  'ColorBurn',
  'HardLight',
  'SoftLight',
  'Difference',
  'Exclusion',
] as const;

const MAX_NAME_LENGTH = 64;

// ─── Props ────────────────────────────────────────────────────────────────────

export interface LayerControlsProps {
  layer: LayerNodeDto;
  onPropsChange: (layerId: number, patch: LayerPropsPatch) => void;
}

// ─── Component ────────────────────────────────────────────────────────────────

/**
 * Per-layer controls: visibility toggle, opacity slider, blend mode dropdown,
 * and editable layer name. Each control calls onPropsChange with only the
 * changed property.
 */
export default function LayerControls({ layer, onPropsChange }: LayerControlsProps) {
  const [editingName, setEditingName] = useState(false);
  const [nameValue, setNameValue] = useState(layer.name);
  const nameInputRef = useRef<HTMLInputElement>(null);

  // ─── Visibility Toggle ────────────────────────────────────────────────

  const handleVisibilityToggle = useCallback(() => {
    onPropsChange(layer.id, { visible: !layer.visible });
  }, [layer.id, layer.visible, onPropsChange]);

  // ─── Opacity Slider ───────────────────────────────────────────────────

  const handleOpacityChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const percent = parseInt(e.target.value, 10);
      const opacity = percent / 100;
      onPropsChange(layer.id, { opacity });
    },
    [layer.id, onPropsChange]
  );

  // ─── Blend Mode Dropdown ──────────────────────────────────────────────

  const handleBlendModeChange = useCallback(
    (e: React.ChangeEvent<HTMLSelectElement>) => {
      onPropsChange(layer.id, { blend_mode: e.target.value });
    },
    [layer.id, onPropsChange]
  );

  // ─── Editable Layer Name ──────────────────────────────────────────────

  const commitName = useCallback(() => {
    const trimmed = nameValue.trim();
    if (trimmed.length > 0 && trimmed.length <= MAX_NAME_LENGTH) {
      if (trimmed !== layer.name) {
        onPropsChange(layer.id, { name: trimmed });
      }
    } else {
      // Reject empty — revert to previous name
      setNameValue(layer.name);
    }
    setEditingName(false);
  }, [nameValue, layer.name, layer.id, onPropsChange]);

  const handleNameDoubleClick = useCallback(() => {
    setNameValue(layer.name);
    setEditingName(true);
    // Focus input on next tick after render
    setTimeout(() => nameInputRef.current?.select(), 0);
  }, [layer.name]);

  const handleNameKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter') {
        commitName();
      } else if (e.key === 'Escape') {
        setNameValue(layer.name);
        setEditingName(false);
      }
    },
    [commitName, layer.name]
  );

  const handleNameChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      // Limit to MAX_NAME_LENGTH chars while typing
      setNameValue(e.target.value.slice(0, MAX_NAME_LENGTH));
    },
    []
  );

  // ─── Render ───────────────────────────────────────────────────────────

  const opacityPercent = Math.round(layer.opacity * 100);

  return (
    <div className="layer-controls" aria-label={`Controls for layer ${layer.name}`}>
      {/* Row 1: Visibility + Name */}
      <div className="layer-controls-row">
        <button
          className={`layer-visibility-btn${layer.visible ? '' : ' layer-visibility-off'}`}
          onClick={handleVisibilityToggle}
          title={layer.visible ? 'Hide layer' : 'Show layer'}
          aria-label={layer.visible ? 'Hide layer' : 'Show layer'}
          aria-pressed={layer.visible}
        >
          {layer.visible ? '👁' : '👁‍🗨'}
        </button>

        {editingName ? (
          <input
            ref={nameInputRef}
            className="layer-name-input"
            type="text"
            value={nameValue}
            onChange={handleNameChange}
            onBlur={commitName}
            onKeyDown={handleNameKeyDown}
            maxLength={MAX_NAME_LENGTH}
            aria-label="Layer name"
          />
        ) : (
          <span
            className="layer-controls-name"
            onDoubleClick={handleNameDoubleClick}
            title="Double-click to rename"
          >
            {layer.name}
          </span>
        )}
      </div>

      {/* Row 2: Opacity */}
      <div className="layer-controls-row">
        <label className="layer-controls-label" htmlFor={`opacity-${layer.id}`}>
          Opacity
        </label>
        <input
          id={`opacity-${layer.id}`}
          className="layer-opacity-slider"
          type="range"
          min={0}
          max={100}
          step={1}
          value={opacityPercent}
          onChange={handleOpacityChange}
          aria-label="Layer opacity"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={opacityPercent}
          aria-valuetext={`${opacityPercent}%`}
        />
        <span className="layer-opacity-value">{opacityPercent}%</span>
      </div>

      {/* Row 3: Blend Mode */}
      <div className="layer-controls-row">
        <label className="layer-controls-label" htmlFor={`blend-${layer.id}`}>
          Blend
        </label>
        <select
          id={`blend-${layer.id}`}
          className="layer-blend-select"
          value={layer.blend_mode}
          onChange={handleBlendModeChange}
          aria-label="Layer blend mode"
        >
          {BLEND_MODES.map((mode) => (
            <option key={mode} value={mode}>
              {mode}
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}
