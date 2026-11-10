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
import { ditherFilterLocksOpacity } from '../features/layers/filterOpacityLock';
import { labelWithShortcut } from '../features/shortcuts/bindings';
import { useShortcutBindings } from '../features/shortcuts/ShortcutsContext';
import {
  sliderEndPadRatio,
  sliderRatioFromClientX,
  sliderThumbLeftCss,
} from '../shared/ui/retroSliderGeometry';
import { isGlitchFamilyFilter } from '../types/effects';
const cn = bind({ ...styles, ...retroSlider, ...layerControls });

/** Same end inset as effect Sliders (see RETRO_SLIDER_END_INSET_PX). */
const OPACITY_SLIDER_PAD = sliderEndPadRatio(160);

// ─── Types ────────────────────────────────────────────────────────────────────

/** Per-document UI state for effect-layer display numbers / rename labels. */
type DocFilterUiState = {
  numbers: Record<string, number>;
  nextNumber: number;
  names: Record<string, string>;
};

function emptyDocFilterUi(): DocFilterUiState {
  return { numbers: {}, nextNumber: 1, names: {} };
}

function docFilterUiKey(docId: number | null | undefined): string {
  return docId == null ? '__none__' : String(docId);
}

export interface LayersPanelProps {
  layers: LayerNodeDto[];
  selectedLayerId: number | null;
  /** Active document — effect `#N` numbering is isolated per open project. */
  docId?: number | null;
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
  /** When true, omit WindowTitlebar — FlexLayout tab strip is the chrome. */
  hideChrome?: boolean;
  onPopOut?: () => void;
  onDockBack?: () => void;
}

// ─── Constants ────────────────────────────────────────────────────────────────

// ─── Helpers ──────────────────────────────────────────────────────────────────

function filterModeFromParams(params: unknown): unknown {
  if (!params || typeof params !== 'object') return undefined;
  const p = params as Record<string, unknown>;
  // Engine may send `{ DitherV2: { mode } }` or a flat params object.
  const inner =
    p.DitherV2 && typeof p.DitherV2 === 'object'
      ? (p.DitherV2 as Record<string, unknown>)
      : p;
  return inner.mode;
}

function filterKindToName(
  kind: string,
  algorithmId?: string | null,
  params?: unknown
): string {
  if (algorithmId === 'nearest_color') return 'Nearest Color';
  if (algorithmId === 'palette_quantize') return 'Palette Quantize';
  if (isGlitchFamilyFilter(kind, algorithmId, filterModeFromParams(params))) {
    return 'Glitching';
  }
  switch (kind) {
    case 'DitherV2': case 'Dither': return 'Dithering';
    case 'Glitch': return 'Glitching';
    case 'Curves': return 'Curves';
    case 'Levels': return 'RGB Channels';
    case 'Glow': return 'Glow';
    case 'Crt': return 'CRT';
    case 'Adjust': return 'Adjust';
    case 'Exposure': return 'Exposure';
    case 'HighlightsShadows': return 'Highlights / Shadows';
    case 'TemperatureTint': return 'Temperature / Tint';
    case 'Vibrance': return 'Vibrance';
    case 'Ascii': return 'ASCII';
    case 'PaletteQuantize': return 'Palette Quantize';
    default: return kind;
  }
}

function filterKindToIconType(
  kind: string,
  algorithmId?: string | null,
  params?: unknown
): string {
  if (algorithmId === 'nearest_color') return 'nearest_color';
  if (algorithmId === 'palette_quantize') return 'palette_quantize';
  if (isGlitchFamilyFilter(kind, algorithmId, filterModeFromParams(params))) {
    return 'glitching';
  }
  switch (kind) {
    case 'DitherV2': case 'Dither': return 'dithering';
    case 'Glitch': return 'glitching';
    case 'Curves': return 'curves';
    case 'Levels': return 'rgb';
    case 'Glow': return 'glow';
    case 'Crt': return 'crt';
    case 'Adjust': return 'adjust';
    case 'Exposure': return 'exposure';
    case 'HighlightsShadows': return 'highlights_shadows';
    case 'TemperatureTint': return 'temperature_tint';
    case 'Vibrance': return 'vibrance';
    case 'Ascii': return 'ascii';
    case 'PaletteQuantize': return 'palette_quantize';
    default: return 'dithering';
  }
}

// ─── Component ────────────────────────────────────────────────────────────────

export default function LayersPanel({
  layers,
  selectedLayerId,
  docId = null,
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
  hideChrome = false,
  onPopOut,
  onDockBack,
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
  const opacityLocked = ditherFilterLocksOpacity(selectedFilter);
  const opacityControlsDisabled = controlsDisabled || opacityLocked;
  const currentOpacityPercent = opacityLocked
    ? 100
    : selectedFilter
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

  // Stable numbering + rename labels, isolated per open document
  const docKey = docFilterUiKey(docId);
  const [filterUiByDoc, setFilterUiByDoc] = useState<Record<string, DocFilterUiState>>({});
  const filterUi = filterUiByDoc[docKey] ?? emptyDocFilterUi();
  const filterNumbers = filterUi.numbers;
  const filterNames = filterUi.names;

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

  // Assign stable numbers to new filters as they appear (within this document only).
  // Skip IDs already numbered under another doc — when open/switch updates docId
  // before refreshFilters settles, the previous project's filters briefly remain
  // in Redux and must not consume this project's #1, #2, …
  useEffect(() => {
    setFilterUiByDoc((prev) => {
      const claimedElsewhere = new Set<string>();
      for (const [key, ui] of Object.entries(prev)) {
        if (key === docKey) continue;
        for (const id of Object.keys(ui.numbers)) {
          claimedElsewhere.add(id);
        }
      }

      const current = prev[docKey] ?? emptyDocFilterUi();
      let nextNumber = current.nextNumber;
      let changed = false;
      const numbers = { ...current.numbers };
      for (const filter of filters) {
        if (claimedElsewhere.has(filter.id)) continue;
        if (!(filter.id in numbers)) {
          numbers[filter.id] = nextNumber++;
          changed = true;
        }
      }
      if (!changed) return prev;
      return {
        ...prev,
        [docKey]: { ...current, numbers, nextNumber },
      };
    });
  }, [filters, docKey]);

  // Drop in-progress rename when switching projects
  useEffect(() => {
    setEditingFilterId(null);
    setEditNameValue('');
  }, [docKey]);

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
      setFilterUiByDoc((prev) => {
        const current = prev[docKey] ?? emptyDocFilterUi();
        return {
          ...prev,
          [docKey]: {
            ...current,
            names: { ...current.names, [editingFilterId]: trimmed },
          },
        };
      });
    }
    setEditingFilterId(null);
    setEditNameValue('');
  }, [docKey, editingFilterId, editNameValue]);

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

  // Snap leftover Fade opacity when entering Strict/Mixed + palette.
  useEffect(() => {
    if (!opacityLocked || selectedFilter == null) return;
    if ((selectedFilter.opacity ?? 1) >= 1) return;
    onFilterBlendChange({ opacity: 1 });
  }, [
    opacityLocked,
    selectedFilter?.id,
    selectedFilter?.opacity,
    onFilterBlendChange,
  ]);

  useEffect(() => {
    if (opacityLocked) {
      setIsOpacityPopupOpen(false);
    }
  }, [opacityLocked]);

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
    if (opacityControlsDisabled) return;
    if (selectedFilterId !== null || selectedLayerId !== null) {
      setIsOpacityPopupOpen((open) => !open);
    }
  }, [opacityControlsDisabled, selectedFilterId, selectedLayerId]);

  const commitOpacity = useCallback(() => {
    if (opacityLocked) {
      setOpacityText('100%');
      setOpacityEditing(false);
      return;
    }
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
  }, [
    opacityLocked,
    opacityText,
    currentOpacityPercent,
    selectedFilterId,
    selectedLayerId,
    onOpacityChange,
    onFilterBlendChange,
  ]);

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
    <div
      className={cn("lp")}
      aria-label="Layers panel"
      data-dock-window="layers"
      data-dock-side={dockSide}
    >
      {!hideChrome && (
        <WindowTitlebar
          title="Layers"
          className={cn("lp-titlebar")}
          onMouseDown={onTitleBarMouseDown}
          dockSide={dockSide}
          onMoveToSide={onMoveToSide}
          onPopOut={onPopOut}
          onDockBack={onDockBack}
        />
      )}

      {/* Blend mode + Opacity row */}
      <div className={cn("lp-controls")}>
        <DropdownMenu
          value={currentBlendMode}
          options={BLEND_MODES.map((mode) => ({ value: mode, label: mode }))}
          onSelect={handleBlendModeChange}
          disabled={controlsDisabled}
          className={cn("lp-dropdown-wrap-small")}
        />

        <div
          className={cn("lp-opacity")}
          title={
            opacityLocked
              ? 'Opacity is locked at 100% in Strict / Mixed — Fade would leave non-palette colors'
              : undefined
          }
        >
          <span className={cn("lp-opacity-label")}>Opacity :</span>
          <div className={cn("lp-opacity-control")} ref={opacityPopupRef}>
            <input
              className={cn("lp-opacity-input")}
              type="text"
              value={opacityText}
              disabled={opacityControlsDisabled}
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
              disabled={opacityControlsDisabled}
              aria-label="Open opacity slider"
            />
            {isOpacityPopupOpen && !opacityLocked && (
              <div className={cn("lp-opacity-popup")}>
                <div
                  className={cn("retro-slider-track", "layer-opacity-slider")}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    const track = e.currentTarget;
                    const updateFromMouse = (clientX: number) => {
                      const rect = track.getBoundingClientRect();
                      const pad = sliderEndPadRatio(rect.width);
                      const ratio = sliderRatioFromClientX(
                        clientX,
                        rect.left,
                        rect.width,
                        pad,
                      );
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
                    style={{
                      left: sliderThumbLeftCss(currentOpacityPercent / 100, OPACITY_SLIDER_PAD),
                    }}
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
            const displayName =
              filterNames[filter.id] ??
              filterKindToName(filter.kind, filter.algorithm_id, filter.params);
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
                    <EffectIconSvg
                      type={filterKindToIconType(
                        filter.kind,
                        filter.algorithm_id,
                        filter.params
                      )}
                    />
                  </div>
                </div>
              </div>
            );
          })}
          {/* Drop indicator at end of list */}
          {dropTargetIndex === filters.length && dragFilterId !== null && (
            <div className={cn("lp-drop-indicator")} />
          )}

          {/* Image Source Layer (always at bottom of stack) */}
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

          {/* Add layer — light gray text under image source */}
          {imageSourceLayer && (
            <button
              type="button"
              className={cn('lp-list-add')}
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                e.preventDefault();
                e.stopPropagation();
                onAddLayer();
              }}
              onClick={(e) => e.preventDefault()}
              aria-label={labelWithShortcut('Add effect', shortcuts.newLayer)}
              title={labelWithShortcut('Add effect', shortcuts.newLayer)}
            >
              <span className={cn('lp-list-add-label')}>add layer</span>
              <span className={cn('lp-list-add-icon')} aria-hidden />
            </button>
          )}

          {/* Empty state */}
          {filters.length === 0 && !imageSourceLayer && (
            <div
              className={cn("lp-add-placeholder")}
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                e.preventDefault();
                e.stopPropagation();
                onAddLayer();
              }}
              onClick={(e) => e.preventDefault()}
            >
              <span>add layer</span>
            </div>
          )}
        </div>
      </SimpleBar>
      </div>

      {/* Footer */}
      <div className={cn("lp-footer")}>
        <Tooltip label={labelWithShortcut('Add effect', shortcuts.newLayer)}>
          <button
            className={cn("lp-footer-btn")}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              e.preventDefault();
              e.stopPropagation();
              onAddLayer();
            }}
            onClick={(e) => e.preventDefault()}
            aria-label={labelWithShortcut('Add effect', shortcuts.newLayer)}
          >
            <Icon name="plus" width={14} height={14} />
          </button>
        </Tooltip>
        <Tooltip label={labelWithShortcut('Delete effect', shortcuts.deleteLayer)}>
          <button
            className={cn("lp-footer-btn")}
            onClick={handleTrashClick}
            disabled={trashDisabled}
            aria-label={labelWithShortcut('Delete effect', shortcuts.deleteLayer)}
          >
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
    case 'exposure':
      return <Icon name="effect.exposure" width={18} height={18} />;
    case 'highlights_shadows':
      return <Icon name="effect.highlights_shadows" width={18} height={18} />;
    case 'temperature_tint':
      return <Icon name="effect.temperature_tint" width={18} height={18} />;
    case 'vibrance':
      return <Icon name="effect.vibrance" width={18} height={18} />;
    case 'ascii':
      return <Icon name="effect.ascii" width={18} height={18} />;
    case 'nearest_color':
      return <Icon name="effect.nearest_color" width={18} height={18} />;
    case 'palette_quantize':
      return <Icon name="effect.palette_quantize" width={18} height={18} />;
    default:
      return <Icon name="effect.dithering" width={18} height={18} />;
  }
}
