import { useState, useCallback, useRef } from 'react';
import ColorPicker from './ColorPicker';
import {
  addColorToPalette,
  updatePaletteColor,
  removePaletteColor,
  reorderPaletteColor,
} from '../ipc/commands';
import styles from './SwatchGrid.module.css';
import { bind } from '../shared/ui/cn';
const cn = bind(styles);

interface SwatchGridProps {
  docId: number;
  paletteId: number;
  colors: string[]; // hex colors (6-char, no "#")
  onColorAdded: () => void;
  onColorUpdated: () => void;
  onColorRemoved: () => void;
  onColorReordered: () => void;
}

function SwatchGrid({
  docId,
  paletteId,
  colors,
  onColorAdded,
  onColorUpdated,
  onColorRemoved,
  onColorReordered,
}: SwatchGridProps) {
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [pickerMode, setPickerMode] = useState<'add' | 'edit' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const dragIndexRef = useRef<number | null>(null);
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null);

  // Click to select
  const handleClick = useCallback((index: number) => {
    setSelectedIndex(index);
    setError(null);
  }, []);

  // Double-click to edit
  const handleDoubleClick = useCallback((index: number) => {
    setSelectedIndex(index);
    setPickerMode('edit');
    setError(null);
  }, []);

  // "+" button opens picker in add mode
  const handleAddClick = useCallback(() => {
    setPickerMode('add');
    setError(null);
  }, []);

  // "−" button removes selected swatch
  const handleRemoveClick = useCallback(async () => {
    if (selectedIndex === null) return;
    setError(null);
    try {
      await removePaletteColor(docId, paletteId, selectedIndex);
      setSelectedIndex(null);
      onColorRemoved();
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    }
  }, [paletteId, selectedIndex, onColorRemoved]);

  // ColorPicker confirm
  const handlePickerConfirm = useCallback(
    async (hex: string) => {
      setError(null);
      try {
        if (pickerMode === 'add') {
          await addColorToPalette(docId, paletteId, hex);
          onColorAdded();
        } else if (pickerMode === 'edit' && selectedIndex !== null) {
          await updatePaletteColor(docId, paletteId, selectedIndex, hex);
          onColorUpdated();
        }
      } catch (e) {
        setError(typeof e === 'string' ? e : String(e));
      }
      setPickerMode(null);
    },
    [paletteId, pickerMode, selectedIndex, onColorAdded, onColorUpdated]
  );

  // ColorPicker cancel
  const handlePickerCancel = useCallback(() => {
    setPickerMode(null);
  }, []);

  // Drag-and-drop handlers
  const handleDragStart = useCallback((e: React.DragEvent, index: number) => {
    dragIndexRef.current = index;
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', String(index));
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent, index: number) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    setDragOverIndex(index);
  }, []);

  const handleDragLeave = useCallback(() => {
    setDragOverIndex(null);
  }, []);

  const handleDrop = useCallback(
    async (e: React.DragEvent, toIndex: number) => {
      e.preventDefault();
      setDragOverIndex(null);
      const fromIndex = dragIndexRef.current;
      dragIndexRef.current = null;

      if (fromIndex === null || fromIndex === toIndex) return;

      setError(null);
      try {
        await reorderPaletteColor(docId, paletteId, fromIndex, toIndex);
        // Update selection to follow the dragged item
        setSelectedIndex(toIndex);
        onColorReordered();
      } catch (e) {
        setError(typeof e === 'string' ? e : String(e));
      }
    },
    [paletteId, onColorReordered]
  );

  const handleDragEnd = useCallback(() => {
    dragIndexRef.current = null;
    setDragOverIndex(null);
  }, []);

  const isEmpty = colors.length === 0;

  return (
    <div className={cn("swatch-grid-container")}>
      {error && (
        <div className={cn("swatch-grid-error")} role="alert">
          {error}
        </div>
      )}

      <div className={cn("swatch-grid")}>
        {colors.map((hex, index) => (
          <div
            key={`${index}-${hex}`}
            className={cn(
              "swatch-item",
              selectedIndex === index && "swatch-selected",
              dragOverIndex === index && "swatch-drag-over"
            )}
            title={hex.toUpperCase()}
            style={{ backgroundColor: `#${hex}` }}
            onClick={() => handleClick(index)}
            onDoubleClick={() => handleDoubleClick(index)}
            draggable
            onDragStart={(e) => handleDragStart(e, index)}
            onDragOver={(e) => handleDragOver(e, index)}
            onDragLeave={handleDragLeave}
            onDrop={(e) => handleDrop(e, index)}
            onDragEnd={handleDragEnd}
            role="button"
            aria-label={`Color swatch ${hex.toUpperCase()}`}
            aria-pressed={selectedIndex === index}
          />
        ))}

        <button
          className={cn("swatch-btn", "swatch-btn-add")}
          onClick={handleAddClick}
          title="Add color"
          aria-label="Add color"
          type="button"
        >
          +
        </button>
      </div>

      <div className={cn("swatch-grid-actions")}>
        <button
          className={cn("swatch-btn", "swatch-btn-remove")}
          onClick={handleRemoveClick}
          disabled={selectedIndex === null || isEmpty}
          title="Remove selected color"
          aria-label="Remove selected color"
          type="button"
        >
          −
        </button>
      </div>

      {pickerMode && (
        <ColorPicker
          initialColor={
            pickerMode === 'edit' && selectedIndex !== null
              ? colors[selectedIndex]
              : undefined
          }
          onConfirm={handlePickerConfirm}
          onCancel={handlePickerCancel}
        />
      )}
    </div>
  );
}

export default SwatchGrid;
