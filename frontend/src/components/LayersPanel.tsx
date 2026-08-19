import type { DockSide } from '../types/panels';
import { useCallback, useState, useEffect, useRef } from 'react';
import type { KeyboardEvent } from 'react';
import type { LayerNodeDto } from '../shared/types/layers';
import type { FilterInfo } from '../types';
import { BLEND_MODES } from '../shared/blendModes';
import DropdownMenu from './common/DropdownMenu';
import SimpleBar from 'simplebar-react';
import WindowTitlebar from '../shared/ui/WindowTitlebar';
import Icon from '../icons/iconRegistry';
import Tooltip from '../shared/ui/Tooltip';
import styles from '../features/layers/LayersPanel.module.css';
import retroSlider from '../shared/ui/RetroSlider.module.css';
import layerControls from '../features/layers/layerControls.module.css';
import { bind } from '../shared/ui/cn';
import {
  displayFilterOrder,
  stackIndexAfterDisplayReorder,
} from '../features/layers/filterDisplayOrder';
import { formatChords } from '../features/shortcuts/bindings';
import { useShortcutBindings } from '../features/shortcuts/ShortcutsContext';
const cn = bind({ ...styles, ...retroSlider, ...layerControls });

// ─── Types ────────────────────────────────────────────────────────────────────

export interface LayersPanelProps {
  layers: LayerNodeDto[];
  selectedLayerId: number | null;
  /** Filters on the image source layer — shown as virtual effect layers */
  filters: FilterInfo[];
  /** Currently selected filter ID (for highlighting) */
  selectedFilterId: string | null;
  onSelect: (id: number) => void;
  onSelectFilter: (filterId: string | null) => void;
  onAddLayer: () => void;
  onRemoveFilter: (filterId: string) => void;
  onReorderFilter: (filterId: string, newIndex: number) => void;
  onToggleVisibility: (id: number) => void;
  onToggleFilterEnabled: (filterId: string) => void;
  onBlendModeChange: (layerId: number, mode: string) => void;
  onOpacityChange: (layerId: number, opacity: number) => void;
  /** Per-filter opacity/blend when a filter row is selected. */
  onFilterBlendChange: (patch: { opacity?: number; blend_mode?: string }) => void;
  /** Mouse down handler for the title bar — used for panel drag-to-reorder/undock */
  onTitleBarMouseDown?: (e: React.MouseEvent) => void;
  dockSide?: DockSide;
  onMoveToSide?: (side: DockSide) => void;
}

// ─── Constants ────────────────────────────────────────────────────────────────

// ─── Helpers ──────────────────────────────────────────────────────────────────

function filterKindToName(kind: string): string {
  switch (kind) {
    case 'DitherV2': case 'Dither': return 'Dithering';
    case 'Glitch': return 'Glitching';
    case 'Curves': return 'Curves';
    case 'Levels': return 'RGB Channels';
    case 'Glow': return 'Glow';
    case 'Crt': return 'CRT';
    case 'Adjust': return 'Adjust';
    default: return kind;
  }
}

function filterKindToIconType(kind: string): string {
  switch (kind) {
    case 'DitherV2': case 'Dither': return 'dithering';
    case 'Glitch': return 'glitching';
    case 'Curves': return 'curves';
    case 'Levels': return 'rgb';
    case 'Glow': return 'glow';
    case 'Crt': return 'crt';
    case 'Adjust': return 'adjust';
    default: return 'dithering';
  }
}

// ─── Component ────────────────────────────────────────────────────────────────

export default function LayersPanel({
  layers,
  selectedLayerId,
  filters,
  selectedFilterId,
  onSelect,
  onSelectFilter,
  onAddLayer,
  onRemoveFilter,
  onReorderFilter,
  onToggleVisibility,
  onToggleFilterEnabled,
  onBlendModeChange,
  onOpacityChange,
  onFilterBlendChange,
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
}: LayersPanelProps) {
  const selectedLayer = selectedLayerId !== null
    ? layers.find(l => l.id === selectedLayerId) ?? null
    : null;

  const imageSourceLayer = layers.length > 0 ? layers[0] : null;
  const shortcuts = useShortcutBindings();
  const displayFilters = displayFilterOrder(filters);
  const selectedFilter =
    selectedFilterId !== null
      ? filters.find((filter) => filter.id === selectedFilterId) ?? null
      : null;
  const trashDisabled = selectedFilterId === null;
  const controlsDisabled = selectedFilter === null && selectedLayerId === null;
  const currentOpacityPercent = selectedFilter
    ? Math.round((selectedFilter.opacity ?? 1) * 100)
    : selectedLayer
      ? Math.round(selectedLayer.opacity * 100)
      : 100;
  const currentBlendMode = selectedFilter
    ? (selectedFilter.blend_mode ?? 'Normal')
    : (selectedLayer?.blend_mode ?? 'Normal');
  const [opacityText, setOpacityText] = useState(`${currentOpacityPercent}%`);
  const [opacityEditing, setOpacityEditing] = useState(false);
  const [isOpacityPopupOpen, setIsOpacityPopupOpen] = useState(false);
  const opacityPopupRef = useRef<HTMLDivElement | null>(null);

  // Stable numbering: assign a persistent number to each filter by ID
  const [filterNumbers, setFilterNumbers] = useState<Record<string, number>>({});
  const nextNumberRef = useRef(1);

  // Custom names for filters (editable via double-click)
  const [filterNames, setFilterNames] = useState<Record<string, string>>({});
  const [editingFilterId, setEditingFilterId] = useState<string | null>(null);
  const [editNameValue, setEditNameValue] = useState('');
  const renameInputRef = useRef<HTMLInputElement>(null);

  // Drag-and-drop state
  const [dragFilterId, setDragFilterId] = useState<string | null>(null);
  const [dropTargetIndex, setDropTargetIndex] = useState<number | null>(null);
  const dragStartY = useRef(0);
  const isDragging = useRef(false);
  const layerListRef = useRef<HTMLDivElement | null>(null);
  const dragFilterIdRef = useRef<string | null>(null);
  const dropTargetIndexRef = useRef<number | null>(null);

  // Assign stable numbers to new filters as they appear
  useEffect(() => {
    let updated = false;
    const newNumbers = { ...filterNumbers };
    for (const filter of filters) {
      if (!(filter.id in newNumbers)) {
        newNumbers[filter.id] = nextNumberRef.current++;
        updated = true;
      }
    }
    if (updated) {
      setFilterNumbers(newNumbers);
    }
  }, [filters]);

  // Focus rename input when editing starts
  useEffect(() => {
    if (editingFilterId !== null && renameInputRef.current) {
      renameInputRef.current.focus();
      renameInputRef.current.select();
    }
  }, [editingFilterId]);

  const handleStartRename = useCallback((filterId: string, currentName: string) => {
    setEditingFilterId(filterId);
    setEditNameValue(currentName);
  }, []);

  const handleCommitRename = useCallback(() => {
    if (editingFilterId === null) return;
    const trimmed = editNameValue.trim();
    if (trimmed.length > 0) {
      setFilterNames((prev) => ({ ...prev, [editingFilterId]: trimmed }));
    }
    setEditingFilterId(null);
    setEditNameValue('');
  }, [editingFilterId, editNameValue]);

  const handleRenameKeyDown = useCallback((e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      handleCommitRename();
    } else if (e.key === 'Escape') {
      setEditingFilterId(null);
      setEditNameValue('');
    }
  }, [handleCommitRename]);

  // ─── Drag-and-Drop Handlers ─────────────────────────────────────────────

  const handleDragMouseDown = useCallback((e: React.MouseEvent, filterId: string) => {
    // Only left mouse button
    if (e.button !== 0) return;
    dragStartY.current = e.clientY;
    isDragging.current = false;
    dragFilterIdRef.current = filterId;
    setDragFilterId(filterId);

    const handleMouseMove = (moveEvent: MouseEvent) => {
      const dy = Math.abs(moveEvent.clientY - dragStartY.current);
      if (dy > 4) {
        isDragging.current = true;
      }
      if (!isDragging.current || !layerListRef.current) return;

      // Determine drop position based on mouse Y relative to rows
      const rows = layerListRef.current.querySelectorAll<HTMLElement>('[data-filter-idx]');
      let targetIdx = filters.length; // default: end
      for (let i = 0; i < rows.length; i++) {
        const rect = rows[i].getBoundingClientRect();
        const midY = rect.top + rect.height / 2;
        if (moveEvent.clientY < midY) {
          targetIdx = i;
          break;
        }
      }
      dropTargetIndexRef.current = targetIdx;
      setDropTargetIndex(targetIdx);
    };

    const handleMouseUp = () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);

      const currentDragId = dragFilterIdRef.current;
      const currentDropIdx = dropTargetIndexRef.current;

      if (isDragging.current && currentDragId !== null && currentDropIdx !== null) {
        const currentIdx = displayFilters.findIndex(f => f.id === currentDragId);
        if (currentIdx >= 0) {
          const stackIdx = stackIndexAfterDisplayReorder(
            filters.length,
            currentIdx,
            currentDropIdx,
          );
          const currentStackIdx = filters.findIndex((f) => f.id === currentDragId);
          if (currentStackIdx !== stackIdx && stackIdx >= 0) {
            onReorderFilter(currentDragId, stackIdx);
          }
        }
      }

      isDragging.current = false;
      dragFilterIdRef.current = null;
      dropTargetIndexRef.current = null;
      setDragFilterId(null);
      setDropTargetIndex(null);
    };

    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
  }, [displayFilters, filters, onReorderFilter]);

  useEffect(() => {
    if (!opacityEditing) {
      setOpacityText(`${currentOpacityPercent}%`);
    }
  }, [currentOpacityPercent, opacityEditing]);

  useEffect(() => {
    if (!isOpacityPopupOpen) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (!opacityPopupRef.current?.contains(event.target as Node)) {
        setIsOpacityPopupOpen(false);
      }
    };
    window.addEventListener('mousedown', handleClickOutside);
    return () => window.removeEventListener('mousedown', handleClickOutside);
  }, [isOpacityPopupOpen]);

  useEffect(() => {
    if (selectedLayerId === null && selectedFilterId === null) {
      setIsOpacityPopupOpen(false);
    }
  }, [selectedLayerId, selectedFilterId]);

  const handleBlendModeChange = useCallback((mode: string) => {
    if (selectedFilterId !== null) {
      onFilterBlendChange({ blend_mode: mode });
      return;
    }
    if (selectedLayerId !== null) {
      onBlendModeChange(selectedLayerId, mode);
    }
  }, [selectedFilterId, selectedLayerId, onBlendModeChange, onFilterBlendChange]);

  const toggleOpacityPopup = useCallback(() => {
    if (selectedFilterId !== null || selectedLayerId !== null) {
      setIsOpacityPopupOpen((open) => !open);
    }
  }, [selectedFilterId, selectedLayerId]);

  const commitOpacity = useCallback(() => {
    const raw = opacityText.replace('%', '').trim();
    const parsed = Number(raw);

    if (!Number.isFinite(parsed)) {
      setOpacityText(`${currentOpacityPercent}%`);
      setOpacityEditing(false);
      return;
    }

    const clamped = Math.min(100, Math.max(0, Math.round(parsed)));
    if (selectedFilterId !== null) {
      onFilterBlendChange({ opacity: clamped / 100 });
    } else if (selectedLayerId !== null) {
      onOpacityChange(selectedLayerId, clamped / 100);
    }
    setOpacityText(`${clamped}%`);
    setOpacityEditing(false);
  }, [opacityText, currentOpacityPercent, selectedFilterId, selectedLayerId, onOpacityChange, onFilterBlendChange]);

  const handleOpacityInputKeyDown = useCallback((e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      commitOpacity();
      (e.target as HTMLInputElement).blur();
    } else if (e.key === 'Escape') {
      setOpacityText(`${currentOpacityPercent}%`);
      setOpacityEditing(false);
      setIsOpacityPopupOpen(false);
      (e.target as HTMLInputElement).blur();
    }
  }, [commitOpacity, currentOpacityPercent]);

  const handleTrashClick = useCallback(() => {
    if (selectedFilterId) {
      onRemoveFilter(selectedFilterId);
    }
  }, [selectedFilterId, onRemoveFilter]);

  return (
    <div className={cn("lp")} aria-label="Layers panel">
      <WindowTitlebar
        title="Layers"
        className={cn("lp-titlebar")}
        onMouseDown={onTitleBarMouseDown}
        dockSide={dockSide}
        onMoveToSide={onMoveToSide}
      />

      {/* Blend mode + Opacity row */}
      <div className={cn("lp-controls")}>
        <DropdownMenu
          value={currentBlendMode}
          options={BLEND_MODES.map((mode) => ({ value: mode, label: mode }))}
          onSelect={handleBlendModeChange}
          disabled={controlsDisabled}
          className={cn("lp-dropdown-wrap-small")}
        />

        <div className={cn("lp-opacity")}>
          <span className={cn("lp-opacity-label")}>Opacity :</span>
          <div className={cn("lp-opacity-control")} ref={opacityPopupRef}>
            <input
              className={cn("lp-opacity-input")}
              type="text"
              value={opacityText}
              disabled={controlsDisabled}
              aria-label="Opacity"
              onChange={(e) => setOpacityText(e.target.value)}
              onFocus={() => setOpacityEditing(true)}
              onBlur={commitOpacity}
              onKeyDown={handleOpacityInputKeyDown}
            />
            <button
              type="button"
              className={cn("lp-opacity-btn")}
              onClick={toggleOpacityPopup}
              disabled={controlsDisabled}
              aria-label="Open opacity slider"
            />
            {isOpacityPopupOpen && (
              <div className={cn("lp-opacity-popup")}>
                <div
                  className={cn("retro-slider-track", "layer-opacity-slider")}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    const track = e.currentTarget;
                    const updateFromMouse = (clientX: number) => {
                      const rect = track.getBoundingClientRect();
                      const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
                      const val = Math.round(ratio * 100);
                      if (selectedFilterId !== null) {
                        onFilterBlendChange({ opacity: val / 100 });
                      } else if (selectedLayerId !== null) {
                        onOpacityChange(selectedLayerId, val / 100);
                      }
                      setOpacityText(`${val}%`);
                    };
                    updateFromMouse(e.clientX);
                    const onMove = (ev: MouseEvent) => updateFromMouse(ev.clientX);
                    const onUp = () => {
                      document.removeEventListener('mousemove', onMove);
                      document.removeEventListener('mouseup', onUp);
                    };
                    document.addEventListener('mousemove', onMove);
                    document.addEventListener('mouseup', onUp);
                  }}
                >
                  <div
                    className={cn("retro-slider-thumb")}
                    style={{ left: `${currentOpacityPercent}%` }}
                  >
                    <img src="/icons/slider-carrete-icon.svg" width="16" height="35" alt="" draggable={false} />
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Layer list — virtual layers (filters) + Image Source */}
      <div className={cn("lp-layers-area")}>
        <SimpleBar style={{ height: '100%' }}>
        <div className={cn("lp-layers-list")} ref={layerListRef}>
          {/* Virtual effect layers (filters displayed as rows) */}
          {displayFilters.map((filter, idx) => {
            const isSelected = filter.id === selectedFilterId;
            const stableNumber = filterNumbers[filter.id] ?? '?';
            const displayName = filterNames[filter.id] ?? filterKindToName(filter.kind);
            const isEditing = editingFilterId === filter.id;
            const isBeingDragged = dragFilterId === filter.id && isDragging.current;
            const showDropBefore = dropTargetIndex === idx && dragFilterId !== null && isDragging.current;

            return (
              <div key={filter.id}>
                {showDropBefore && <div className={cn("lp-drop-indicator")} />}
                <div
                  className={cn(
                    'lp-layer-row',
                    isSelected && 'lp-layer-row-selected',
                    isBeingDragged && 'lp-layer-row-dragging'
                  )}
                  data-filter-idx={idx}
                  onClick={() => {
                    if (!isDragging.current) onSelectFilter(filter.id);
                  }}
                  onMouseDown={(e) => handleDragMouseDown(e, filter.id)}
                  role="treeitem"
                  aria-selected={isSelected}
                  style={{ cursor: dragFilterId ? 'grabbing' : 'grab' }}
                >
                  <Tooltip label={filter.enabled ? 'Hide' : 'Show'}>
                    <button
                      className={cn("lp-eye-btn")}
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onToggleFilterEnabled(filter.id);
                      }}
                      aria-label={filter.enabled ? 'Hide effect' : 'Show effect'}
                    >
                      <Icon name="open-eye" width={18} height={18} style={{ opacity: filter.enabled ? 1 : 0.3 }} />
                    </button>
                  </Tooltip>

                  <div className={cn("lp-layer-name")}>
                    {isEditing ? (
                      <input
                        ref={renameInputRef}
                        type="text"
                        className={cn("lp-layer-name-input")}
                        value={editNameValue}
                        onChange={(e) => setEditNameValue(e.target.value)}
                        onKeyDown={handleRenameKeyDown}
                        onBlur={handleCommitRename}
                        onClick={(e) => e.stopPropagation()}
                        onMouseDown={(e) => e.stopPropagation()}
                        aria-label="Rename layer"
                      />
                    ) : (
                      <span
                        onDoubleClick={(e) => {
                          e.stopPropagation();
                          handleStartRename(filter.id, displayName);
                        }}
                        title="Double-click to rename"
                      >
                        #{stableNumber} {displayName}
                      </span>
                    )}
                  </div>

                  <div className={cn("lp-layer-icon")}>
                    <EffectIconSvg type={filterKindToIconType(filter.kind)} />
                  </div>
                </div>
              </div>
            );
          })}
          {/* Drop indicator at end of list */}
          {dropTargetIndex === filters.length && dragFilterId !== null && (
            <div className={cn("lp-drop-indicator")} />
          )}

          {/* Image Source Layer (always at bottom) */}
          {imageSourceLayer && (
            <div
              className={cn(
                'lp-layer-row',
                selectedFilterId === null && selectedLayerId === imageSourceLayer.id && 'lp-layer-row-selected'
              )}
              onClick={() => { onSelect(imageSourceLayer.id); }}
              role="treeitem"
            >
              <Tooltip label={imageSourceLayer.visible ? 'Hide' : 'Show'}>
                <button
                  className={cn("lp-eye-btn")}
                  type="button"
                  onClick={(e) => { e.stopPropagation(); onToggleVisibility(imageSourceLayer.id); }}
                  aria-label={imageSourceLayer.visible ? 'Hide layer' : 'Show layer'}
                >
                  <Icon name="open-eye" width={18} height={18} style={{ opacity: imageSourceLayer.visible ? 1 : 0.3 }} />
                </button>
              </Tooltip>
              <div className={cn("lp-layer-name")}>
                <span>Image Source</span>
              </div>
              <div className={cn("lp-layer-icon")}>
                <Icon name="image.source" width={18} height={18} />
              </div>
            </div>
          )}

          {/* Empty state */}
          {filters.length === 0 && !imageSourceLayer && (
            <div className={cn("lp-add-placeholder")} onClick={onAddLayer}>
              <span>add layer</span>
            </div>
          )}
        </div>
      </SimpleBar>
      </div>

      {/* Footer */}
      <div className={cn("lp-footer")}>
        <Tooltip label={`Add effect (${formatChords(shortcuts.newLayer)})`}>
          <button className={cn("lp-footer-btn")} onClick={onAddLayer} aria-label="Add effect">
            <Icon name="plus" width={14} height={14} />
          </button>
        </Tooltip>
        <Tooltip label={`Delete effect (${formatChords(shortcuts.deleteLayer)})`}>
          <button className={cn("lp-footer-btn")} onClick={handleTrashClick} disabled={trashDisabled} aria-label="Delete effect">
            <Icon name="trash" width={14} height={14} />
          </button>
        </Tooltip>
      </div>
    </div>
  );
}

// ─── Effect Icon SVG ──────────────────────────────────────────────────────────

function EffectIconSvg({ type }: { type: string }) {
  switch (type) {
    case 'dithering':
      return <Icon name="effect.dithering" width={18} height={18} />;
    case 'glitching':
      return <Icon name="effect.glitching" width={18} height={18} />;
    case 'curves':
      return <Icon name="effect.curves" width={18} height={18} />;
    case 'rgb':
      return <Icon name="effect.rgb" width={18} height={18} />;
    case 'glow':
      return <Icon name="effect.glow" width={18} height={18} />;
    case 'crt':
      return <Icon name="effect.crt" width={18} height={18} />;
    case 'adjust':
      return <Icon name="effect.adjust" width={18} height={18} />;
    default:
      return <Icon name="effect.dithering" width={18} height={18} />;
  }
}
