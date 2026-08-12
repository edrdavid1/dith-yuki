import { useCallback, useEffect, useState } from 'react';
import SimpleBar from 'simplebar-react';
import ColorPicker from '../../components/ColorPicker';
import WindowTitlebar from '../../shared/ui/WindowTitlebar';
import { useAppDispatch, useAppSelector } from '../../app/hooks';
import { bumpVersion } from '../../app/slices/palettesSlice';
import {
  addColor,
  deleteColor,
  resetDraft,
  setColorAt,
  setColors,
  setError,
  setExtractCount,
  setExtractMethod,
  setName,
  setSuccessMessage,
} from '../../app/slices/colorLabSlice';
import { extractPalette } from '../../app/autoExtract';
import {
  addPalette,
  emitPaletteChanged,
  exportPalette,
  formatIpcError,
  importBuiltinPalette,
  importPalette,
  listPalettes,
  logIpcError,
  type PaletteDto,
} from '../../shared/ipc';
import { openDialog, saveDialog } from '../../shared/ipc/dialogs';
import { toHex, sortByBrightness } from '../../types/effects';
import type { PanelChromeProps } from '../panels/PanelChrome';
import ColorLabBody, { type ColorLabVariant } from './ColorLabBody';
import { createColorEntry, MAX_COLORS } from './types';
import { useColorLabDraftSync } from './useColorLabDraftSync';
import styles from './ColorLabWindow.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

export type { ColorLabVariant };

export type ColorLabFeatureProps = PanelChromeProps & {
  variant: ColorLabVariant;
  /** Show in-panel titlebar (docked sidebar). Floating shell has its own chrome. */
  showTitlebar?: boolean;
};

/**
 * Connected Color Lab — shared draft in RTK; sidebar (compact) and full window stay synced.
 */
export default function ColorLabFeature({
  variant,
  showTitlebar = false,
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
}: ColorLabFeatureProps) {
  const dispatch = useAppDispatch();
  const hasDocument = useAppSelector((s) => s.document.hasDocument);
  const layerId = useAppSelector((s) => s.selection.layerId);
  const palettesVersion = useAppSelector((s) => s.palettes.version);
  const { name, colors, extractMethod, extractCount, error, successMessage } = useAppSelector(
    (s) => s.colorLab
  );

  useColorLabDraftSync();

  const [palettes, setPalettes] = useState<PaletteDto[]>([]);
  const [selectedPaletteId, setSelectedPaletteId] = useState<number | null>(null);
  const [colorPickerIndex, setColorPickerIndex] = useState<number | null>(null);
  const [pickerAnchorRect, setPickerAnchorRect] = useState<DOMRect | null>(null);

  useEffect(() => {
    let cancelled = false;
    listPalettes()
      .then((list) => {
        if (!cancelled) setPalettes(list);
      })
      .catch((err) => {
        if (!cancelled) logIpcError('ColorLabFeature.listPalettes', err);
      });
    return () => {
      cancelled = true;
    };
  }, [palettesVersion]);

  const handleExtract = useCallback(async () => {
    if (!hasDocument || layerId === null) {
      dispatch(setError('No image loaded — cannot extract palette.'));
      return;
    }
    const result = await dispatch(extractPalette({ layerId }));
    if (extractPalette.fulfilled.match(result)) {
      setSelectedPaletteId(null);
    }
  }, [dispatch, hasDocument, layerId]);


  const handleSelectPalette = useCallback(
    (paletteId: number) => {
      const palette = palettes.find((p) => p.id === paletteId);
      if (!palette) return;
      setSelectedPaletteId(paletteId);
      dispatch(setName(palette.name));
      dispatch(
        setColors(
          palette.colors.map(([r, g, b]) => createColorEntry(toHex(r, g, b)))
        )
      );
      dispatch(setError(null));
    },
    [dispatch, palettes]
  );

  const handleImport = useCallback(async () => {
    try {
      const filePath = await openDialog({
        multiple: false,
        filters: [{ name: 'Palettes', extensions: ['ase', 'aco', 'gpl', 'pal', 'csv', 'json'] }],
      });
      if (!filePath) return;
      const dto = await importPalette(filePath as string);
      dispatch(setColors(dto.colors.map(([r, g, b]) => createColorEntry(toHex(r, g, b)))));
      if (dto.name) dispatch(setName(dto.name));
      setSelectedPaletteId(dto.id ?? null);
      dispatch(bumpVersion({ lastCreatedId: dto.id }));
      dispatch(setError(null));
    } catch (err: unknown) {
      dispatch(setError(formatIpcError(err)));
      logIpcError('ColorLabFeature.import', err);
    }
  }, [dispatch]);

  const handleImportBuiltin = useCallback(
    async (id: string) => {
      try {
        const dto = await importBuiltinPalette(id);
        dispatch(setColors(dto.colors.map(([r, g, b]) => createColorEntry(toHex(r, g, b)))));
        if (dto.name) dispatch(setName(dto.name));
        setSelectedPaletteId(dto.id ?? null);
        dispatch(bumpVersion({ lastCreatedId: dto.id }));
        void emitPaletteChanged();
        dispatch(setError(null));
      } catch (err: unknown) {
        dispatch(setError(formatIpcError(err)));
        logIpcError('ColorLabFeature.importBuiltin', err);
      }
    },
    [dispatch]
  );

  const handleInsertGeneratedColors = useCallback(
    (hexColors: string[]) => {
      // Replace draft colors with generated ramp/harmony (document only on Apply)
      dispatch(setColors(hexColors.map((hex) => createColorEntry(hex.startsWith('#') ? hex : `#${hex}`))));
      setSelectedPaletteId(null);
      dispatch(setError(null));
    },
    [dispatch]
  );

  const handleGeneratorError = useCallback(
    (message: string | null) => {
      dispatch(setError(message));
    },
    [dispatch]
  );

  const handleExport = useCallback(
    async (format = 'gpl') => {
      const validColors = colors.filter((c) => c.valid);
      if (validColors.length === 0) {
        dispatch(setError('No colors to export.'));
        return;
      }
      const formatLower = (format || 'gpl').toLowerCase();
      const extension = formatLower === 'hex' ? 'pal' : formatLower;
      try {
        const savePath = await saveDialog({
          filters: [{ name: format.toUpperCase() || 'GPL', extensions: [extension] }],
        });
        if (!savePath) return;
        const rgbTuples = validColors.map((c) => [c.r, c.g, c.b] as [number, number, number]);
        const dto = await addPalette(name.trim() || 'Export', rgbTuples);
        await exportPalette(dto.id, savePath, formatLower);
        dispatch(setError(null));
        dispatch(setSuccessMessage('Palette exported successfully.'));
        window.setTimeout(() => dispatch(setSuccessMessage(null)), 3000);
      } catch (err: unknown) {
        dispatch(setError(formatIpcError(err)));
        logIpcError('ColorLabFeature.export', err);
      }
    },
    [colors, dispatch, name]
  );

  const handleSortByBrightness = useCallback(() => {
    const validColors = colors.filter((c) => c.valid);
    const invalidColors = colors.filter((c) => !c.valid);
    const sorted = sortByBrightness(
      validColors.map((c) => [c.r, c.g, c.b] as [number, number, number])
    );
    dispatch(
      setColors([
        ...sorted.map(([r, g, b]) => createColorEntry(toHex(r, g, b))),
        ...invalidColors,
      ])
    );
  }, [colors, dispatch]);

  const handleApply = useCallback(async () => {
    if (!name.trim()) {
      dispatch(setError('Palette name cannot be empty.'));
      return;
    }
    if (colors.some((c) => !c.valid)) {
      dispatch(setError('Fix invalid hex values before applying.'));
      return;
    }
    const validColors = colors.filter((c) => c.valid);
    if (validColors.length === 0) {
      dispatch(setError('No valid colors to save.'));
      return;
    }
    try {
      const dto = await addPalette(
        name.trim(),
        validColors.map((c) => [c.r, c.g, c.b] as [number, number, number])
      );
      dispatch(bumpVersion({ lastCreatedId: dto.id }));
      setSelectedPaletteId(dto.id);
      void emitPaletteChanged();
      dispatch(setError(null));
      dispatch(setSuccessMessage('Palette saved.'));
      window.setTimeout(() => dispatch(setSuccessMessage(null)), 3000);
    } catch (err: unknown) {
      dispatch(setError(formatIpcError(err)));
      logIpcError('ColorLabFeature.apply', err);
    }
  }, [colors, dispatch, name]);

  const handleOpenColorPicker = useCallback((index: number, e: React.MouseEvent) => {
    setColorPickerIndex(index);
    setPickerAnchorRect((e.currentTarget as HTMLElement).getBoundingClientRect());
  }, []);

  const handleCloseColorPicker = useCallback(() => {
    setColorPickerIndex(null);
    setPickerAnchorRect(null);
  }, []);

  const handleColorPickerConfirm = useCallback(
    (hex: string) => {
      if (colorPickerIndex === null) return;
      const formatted = hex.startsWith('#') ? hex : `#${hex}`;
      dispatch(setColorAt({ index: colorPickerIndex, hex: formatted }));
    },
    [colorPickerIndex, dispatch]
  );

  const body = (
    <ColorLabBody
      variant={variant}
      name={name}
      onNameChange={(v) => {
        dispatch(setName(v));
        setSelectedPaletteId(null);
      }}
      paletteOptions={palettes.map((p) => ({ id: p.id, name: p.name }))}
      selectedPaletteId={selectedPaletteId}
      onSelectPalette={handleSelectPalette}
      extractMethod={extractMethod}
      extractCount={extractCount}
      onMethodChange={(m) => dispatch(setExtractMethod(m))}
      onCountChange={(n) => dispatch(setExtractCount(n))}
      onExtractRaw={handleExtract}
      onExtractActual={handleExtract}
      colors={colors}
      canAddColor={colors.length < MAX_COLORS}
      onColorChange={(index, hex) => dispatch(setColorAt({ index, hex }))}
      onDeleteColor={(index) => dispatch(deleteColor(index))}
      onAddColor={() => dispatch(addColor(undefined))}
      onOpenPicker={handleOpenColorPicker}
      error={error}
      successMessage={successMessage}
      onSort={handleSortByBrightness}
      onReset={() => {
        dispatch(resetDraft());
        setSelectedPaletteId(null);
      }}
      onApply={handleApply}
      onImport={handleImport}
      onImportBuiltin={handleImportBuiltin}
      onInsertGeneratedColors={handleInsertGeneratedColors}
      onGeneratorError={handleGeneratorError}
      onExport={handleExport}
    />
  );

  const picker =
    colorPickerIndex !== null && colors[colorPickerIndex] ? (
      <ColorPicker
        initialColor={colors[colorPickerIndex].hex.replace('#', '')}
        onConfirm={handleColorPickerConfirm}
        onCancel={handleCloseColorPicker}
        anchorRect={pickerAnchorRect}
      />
    ) : null;

  if (variant === 'sidebar') {
    return (
      <div className={cn('color-lab-sidebar')}>
        {showTitlebar && (
          <WindowTitlebar
            title="Color Lab"
            onMouseDown={onTitleBarMouseDown}
            dockSide={dockSide}
            onMoveToSide={onMoveToSide}
          />
        )}
        <div className={cn('color-lab-scroll')}>
          <SimpleBar style={{ height: '100%' }}>{body}</SimpleBar>
        </div>
        {picker}
      </div>
    );
  }

  return (
    <div className={cn('color-lab-floating')}>
      {body}
      {picker}
    </div>
  );
}
