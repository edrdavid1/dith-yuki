import AutoExtractSection from './AutoExtractSection';
import HarmonySection from './HarmonySection';
import ImportExportSection from './ImportExportSection';
import PaletteManualEditor from './PaletteManualEditor';
import PaletteManagerSection from './PaletteManagerSection';
import PaletteVolumeViewer from './PaletteVolumeViewer';
import RampGeneratorSection from './RampGeneratorSection';
import ColorLabFooter from './ColorLabFooter';
import type { ColorEntry, ExtractMethod } from './types';
import type { BuiltinPaletteDto, PaletteDto } from '../../shared/ipc';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

export type ColorLabVariant = 'sidebar' | 'full';

export interface ColorLabBodyProps {
  variant: ColorLabVariant;
  name: string;
  onNameChange: (name: string) => void;
  palettes: PaletteDto[];
  builtins: BuiltinPaletteDto[];
  selectedPaletteId: number | null;
  onSelectPalette: (paletteId: number) => void;
  onSelectBuiltin: (id: string) => void;
  onSelectNew: () => void;
  extractMethod: ExtractMethod;
  extractCount: number;
  chromaWeight: number;
  contrastWeight: number;
  onMethodChange: (method: ExtractMethod) => void;
  onCountChange: (count: number) => void;
  onChromaWeightChange: (value: number) => void;
  onContrastWeightChange: (value: number) => void;
  onExtractRaw: () => void;
  onExtractActual: () => void;
  colors: ColorEntry[];
  selectedColorIndex: number | null;
  onSelectColor: (index: number) => void;
  canAddColor: boolean;
  onColorChange: (index: number, hex: string) => void;
  onDeleteColor: (index: number) => void;
  onAddColor: () => void;
  onOpenPicker: (index: number, e: React.MouseEvent) => void;
  error: string | null;
  successMessage: string | null;
  onSort: () => void;
  canAutoInterpolate?: boolean;
  onAutoInterpolate?: () => void;
  onReset: () => void;
  onApply: () => void;
  onImport: () => void;
  onExport: (format?: string) => void;
  onDeleteSaved: (id: number) => void;
  onExportSaved: (id: number) => void;
  onInsertGeneratedColors: (hexColors: string[]) => void;
  onGeneratorError: (message: string | null) => void;
}

const PLACEHOLDER = 'name-of-saved-palette';

/** Full Color Lab body. Docked (`sidebar`) is the same tools, one column. */
export default function ColorLabBody(props: ColorLabBodyProps) {
  const isSidebar = props.variant === 'sidebar';

  return (
    <div className={cn('color-lab-body', isSidebar && 'color-lab-body-sidebar')}>
      <PaletteManagerSection
        builtins={props.builtins}
        saved={props.palettes}
        selectedPaletteId={props.selectedPaletteId}
        onSelectNew={props.onSelectNew}
        onSelectSaved={props.onSelectPalette}
        onSelectBuiltin={props.onSelectBuiltin}
        onDeleteSaved={props.onDeleteSaved}
        onExportSaved={props.onExportSaved}
        onImport={props.onImport}
        previewColors={props.colors
          .filter((c) => c.valid)
          .map((c) => [c.r, c.g, c.b] as [number, number, number])}
      />

      <AutoExtractSection
        extractMethod={props.extractMethod}
        extractCount={props.extractCount}
        chromaWeight={props.chromaWeight}
        contrastWeight={props.contrastWeight}
        onMethodChange={props.onMethodChange}
        onCountChange={props.onCountChange}
        onChromaWeightChange={props.onChromaWeightChange}
        onContrastWeightChange={props.onContrastWeightChange}
        onExtractRaw={props.onExtractRaw}
        onExtractActual={props.onExtractActual}
      />

      <input
        type="text"
        className={cn('color-lab-name-input')}
        value={props.name}
        onChange={(e) => props.onNameChange(e.target.value)}
        placeholder={PLACEHOLDER}
        aria-label="Palette name"
      />

      <PaletteManualEditor
        colors={props.colors}
        canAddColor={props.canAddColor}
        compact={isSidebar}
        showSectionTitle
        selectedIndex={props.selectedColorIndex}
        onSelect={props.onSelectColor}
        onChange={props.onColorChange}
        onDelete={props.onDeleteColor}
        onAdd={props.onAddColor}
        onOpenPicker={props.onOpenPicker}
      />

      {(props.error || props.successMessage) && (
        <>
          {props.error && <div className={cn('color-lab-error')}>{props.error}</div>}
          {props.successMessage && (
            <div className={cn('color-lab-success')}>{props.successMessage}</div>
          )}
        </>
      )}

      <ColorLabFooter
        cancelLabel="Reset"
        onSort={props.onSort}
        canAutoInterpolate={props.canAutoInterpolate}
        onAutoInterpolate={props.onAutoInterpolate}
        onCancel={props.onReset}
        onApply={props.onApply}
      />

      <div className={cn('color-lab-stack')}>
        <RampGeneratorSection
          onInsert={props.onInsertGeneratedColors}
          onError={props.onGeneratorError}
        />
        <HarmonySection
          onInsert={props.onInsertGeneratedColors}
          onError={props.onGeneratorError}
        />
      </div>

      <PaletteVolumeViewer
        colors={props.colors}
        selectedIndex={props.selectedColorIndex}
        onSelectIndex={props.onSelectColor}
        compact={isSidebar}
      />

      <ImportExportSection
        canExport={props.colors.length > 0}
        onImport={props.onImport}
        onExport={props.onExport}
      />
    </div>
  );
}
