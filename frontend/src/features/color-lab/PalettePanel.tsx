import { useState, useEffect, useCallback, useRef } from 'react';
import { useAppSelector } from '../../app/hooks';
import {
  listPalettes,
  importPalette,
  generatePalette,
  createPalette,
  deletePalette,
  renamePalette,
  exportPalette,
} from '../../shared/ipc';
import type { PaletteDto } from '../../shared/ipc';
import { openDialog as open, saveDialog as save } from '../../shared/ipc/dialogs';
import SwatchGrid from '../../components/SwatchGrid';
import DropdownMenu from '../../components/common/DropdownMenu';
import styles from './PalettePanel.module.css';
import buttonStyles from '../../shared/ui/FilterButtons.module.css';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import inputStyles from '../../shared/ui/ParamInput.module.css';
import sliderStyles from '../../shared/ui/Slider.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles, ...paramStyles, ...inputStyles, ...sliderStyles });

interface PalettePanelProps {
  layerId: number | null;
}

function PalettePanel({ layerId }: PalettePanelProps) {
  const docId = useAppSelector((s) => s.document.docId);
  const [palettes, setPalettes] = useState<PaletteDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [genMethod, setGenMethod] = useState<string>('MedianCut');
  const [genCount, setGenCount] = useState<number>(16);
  const [showGenOptions, setShowGenOptions] = useState(false);
  const [selectedPaletteId, setSelectedPaletteId] = useState<number | null>(null);

  // Create palette state
  const [showCreateInput, setShowCreateInput] = useState(false);
  const [createName, setCreateName] = useState('');
  const [createError, setCreateError] = useState<string | null>(null);
  const createInputRef = useRef<HTMLInputElement>(null);

  // Inline rename state
  const [editingPaletteId, setEditingPaletteId] = useState<number | null>(null);
  const [editName, setEditName] = useState('');
  const renameInputRef = useRef<HTMLInputElement>(null);

  const refresh = useCallback(() => {
    listPalettes().then(setPalettes).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Focus create input when it appears
  useEffect(() => {
    if (showCreateInput && createInputRef.current) {
      createInputRef.current.focus();
    }
  }, [showCreateInput]);

  // Focus rename input when editing starts
  useEffect(() => {
    if (editingPaletteId !== null && renameInputRef.current) {
      renameInputRef.current.focus();
      renameInputRef.current.select();
    }
  }, [editingPaletteId]);

  // ─── Create Palette ──────────────────────────────────────────────────────────

  const handleCreateClick = () => {
    setShowCreateInput(true);
    setCreateName('');
    setCreateError(null);
  };

  const handleCreateSubmit = async () => {
    const trimmed = createName.trim();
    if (trimmed.length === 0 || trimmed.length > 255) {
      setCreateError('Name must be between 1 and 255 characters');
      return;
    }
    setCreateError(null);
    try {
      if (docId == null) return;
      await createPalette(docId, trimmed);
      setShowCreateInput(false);
      setCreateName('');
      refresh();
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    }
  };

  const handleCreateKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleCreateSubmit();
    } else if (e.key === 'Escape') {
      setShowCreateInput(false);
      setCreateName('');
      setCreateError(null);
    }
  };

  // ─── Import ──────────────────────────────────────────────────────────────────

  const handleImport = async () => {
    setError(null);
    try {
      const selected = await open({
        filters: [{
          name: 'Palettes',
          extensions: ['ase', 'aco', 'gpl', 'pal', 'csv', 'json'],
        }],
        multiple: false,
      });
      if (selected && typeof selected === 'string') {
        if (docId == null) return;
        await importPalette(docId, selected);
        refresh();
      }
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    }
  };

  // ─── Export ──────────────────────────────────────────────────────────────────

  const handleExport = async (paletteId: number) => {
    setError(null);
    try {
      const filePath = await save({
        filters: [
          { name: 'Adobe Swatch Exchange', extensions: ['ase'] },
          { name: 'GIMP Palette', extensions: ['gpl'] },
          { name: 'JSON', extensions: ['json'] },
          { name: 'Adobe Color', extensions: ['aco'] },
          { name: 'Microsoft RIFF', extensions: ['pal'] },
          { name: 'CSV', extensions: ['csv'] },
        ],
      });

      if (!filePath) return; // User cancelled

      // Derive format from file extension
      const ext = filePath.split('.').pop()?.toLowerCase() ?? '';
      const supportedFormats = ['ase', 'gpl', 'json', 'aco', 'pal', 'csv'];
      const format = supportedFormats.includes(ext) ? ext : 'json';

      if (docId == null) return;
      await exportPalette(docId, paletteId, filePath, format);
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    }
  };

  // ─── Delete ──────────────────────────────────────────────────────────────────

  const handleDelete = async () => {
    if (selectedPaletteId === null) return;
    const palette = palettes.find((p) => p.id === selectedPaletteId);
    if (!palette) return;

    const confirmed = window.confirm(`Delete palette "${palette.name}"? This cannot be undone.`);
    if (!confirmed) return;

    setError(null);
    try {
      if (docId == null) return;
      await deletePalette(docId, selectedPaletteId);
      setSelectedPaletteId(null);
      refresh();
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    }
  };

  // ─── Generate ────────────────────────────────────────────────────────────────

  const handleGenerate = async () => {
    if (layerId === null) {
      setError('No layer selected');
      return;
    }
    setError(null);
    try {
      if (docId == null || layerId == null) return;
      await generatePalette(docId, layerId, genCount, genMethod);
      refresh();
      setShowGenOptions(false);
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    }
  };

  // ─── Inline Rename ───────────────────────────────────────────────────────────

  const handleStartRename = (palette: PaletteDto) => {
    setEditingPaletteId(palette.id);
    setEditName(palette.name);
  };

  const handleRenameSubmit = async () => {
    if (editingPaletteId === null) return;
    const trimmed = editName.trim();
    if (trimmed.length === 0 || trimmed.length > 255) {
      // Invalid name, revert
      setEditingPaletteId(null);
      setEditName('');
      return;
    }
    try {
      if (docId == null) return;
      await renamePalette(docId, editingPaletteId, trimmed);
      refresh();
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    }
    setEditingPaletteId(null);
    setEditName('');
  };

  const handleRenameKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleRenameSubmit();
    } else if (e.key === 'Escape') {
      setEditingPaletteId(null);
      setEditName('');
    }
  };

  // ─── Selection ───────────────────────────────────────────────────────────────

  const handleSelectPalette = (paletteId: number) => {
    setSelectedPaletteId(paletteId === selectedPaletteId ? null : paletteId);
  };

  const selectedPalette = palettes.find((p) => p.id === selectedPaletteId) ?? null;

  return (
    <>
      {/* Action buttons */}
      <div className={cn("sidebar-section")}>
        <div className={cn("filter-add-buttons")}>
          <button className={cn("filter-add-btn")} onClick={handleCreateClick}>
            Create
          </button>
          <button className={cn("filter-add-btn")} onClick={handleImport}>
            Import
          </button>
          <button
            className={cn("filter-add-btn")}
            onClick={() => handleExport(selectedPaletteId!)}
            disabled={selectedPaletteId === null}
          >
            Export
          </button>
          <button
            className={cn("filter-add-btn")}
            onClick={handleDelete}
            disabled={selectedPaletteId === null}
          >
            Delete
          </button>
          <button className={cn("filter-add-btn")} onClick={() => setShowGenOptions(!showGenOptions)}>
            Generate
          </button>
        </div>
      </div>

      {/* Create palette name input */}
      {showCreateInput && (
        <div className={cn("sidebar-section", "palette-panel-section")}>
          <input
            ref={createInputRef}
            type="text"
            value={createName}
            onChange={(e) => setCreateName(e.target.value)}
            onKeyDown={handleCreateKeyDown}
            onBlur={handleCreateSubmit}
            placeholder="Palette name..."
            className={cn("palette-panel-name-input")}
            aria-label="New palette name"
          />
          {createError && (
            <div style={{ color: '#c00', fontSize: '10px', marginTop: '2px' }}>
              {createError}
            </div>
          )}
        </div>
      )}

      {/* Generate options */}
      {showGenOptions && (
        <div className={cn("sidebar-section", "palette-panel-section-divider")}>
          <DropdownMenu
            label="Method"
            value={genMethod}
            options={[
              { value: 'MedianCut', label: 'Median Cut' },
              { value: 'KMeans', label: 'K-Means' },
            ]}
            onSelect={(v) => setGenMethod(v)}
          />
          <div className={cn("param-group")}>
            <label className={cn("slider-label")}>Colors</label>
            <input
              type="number"
              className={cn("param-input")}
              value={genCount}
              min={2}
              max={256}
              onChange={(e) => setGenCount(Math.max(2, Math.min(256, Number(e.target.value))))}
              style={{ width: '60px', fontSize: '11px', padding: '2px 4px' }}
            />
          </div>
          <button className={cn("filter-add-btn")} onClick={handleGenerate} style={{ marginTop: '4px' }}>
            Generate from Layer
          </button>
        </div>
      )}

      {/* Error display */}
      {error && (
        <div style={{ color: '#c00', fontSize: '10px', padding: '2px 8px' }}>{error}</div>
      )}

      {/* Palette list */}
      {palettes.length > 0 && (
        <div className={cn("sidebar-section", "palette-panel-list")}>
          {palettes.map((palette) => {
            const isSelected = palette.id === selectedPaletteId;
            const isEditing = palette.id === editingPaletteId;

            return (
              <div
                key={palette.id}
                onClick={() => handleSelectPalette(palette.id)}
                className={cn("palette-panel-item", isSelected && "palette-panel-item-selected")}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '2px' }}>
                  {isEditing ? (
                    <input
                      ref={renameInputRef}
                      type="text"
                      value={editName}
                      onChange={(e) => setEditName(e.target.value)}
                      onKeyDown={handleRenameKeyDown}
                      onBlur={handleRenameSubmit}
                      onClick={(e) => e.stopPropagation()}
                      className={cn("palette-panel-rename-input")}
                      aria-label="Rename palette"
                    />
                  ) : (
                    <span
                      style={{ fontSize: '11px', fontWeight: 'bold' }}
                      onDoubleClick={(e) => {
                        e.stopPropagation();
                        handleStartRename(palette);
                      }}
                      title="Double-click to rename"
                    >
                      {palette.name}
                    </span>
                  )}
                </div>
                {/* Color preview: first 8 colors as 16x16 swatches */}
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '1px' }}>
                  {palette.hex_colors.slice(0, 8).map((hex, idx) => (
                    <div
                      key={idx}
                      title={hex}
                      className={cn("palette-panel-preview-swatch", isSelected && "palette-panel-preview-swatch-selected")}
                      style={{ backgroundColor: `#${hex}` }}
                    />
                  ))}
                  {palette.color_count > 8 && (
                    <span style={{ fontSize: '9px', color: isSelected ? '#ccc' : '#666', alignSelf: 'center', marginLeft: '4px' }}>
                      +{palette.color_count - 8}
                    </span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {palettes.length === 0 && !error && (
        <div style={{ color: '#666', fontSize: '10px', padding: '4px 8px' }}>
          No palettes. Create, import, or generate one.
        </div>
      )}

      {/* SwatchGrid for selected palette */}
      {selectedPalette && (
        <div className={cn("sidebar-section", "palette-panel-section-divider-padded")}>
          <SwatchGrid
              docId={docId!}
            paletteId={selectedPalette.id}
            colors={selectedPalette.hex_colors}
            onColorAdded={refresh}
            onColorUpdated={refresh}
            onColorRemoved={refresh}
            onColorReordered={refresh}
          />
        </div>
      )}
    </>
  );
}

export default PalettePanel;
