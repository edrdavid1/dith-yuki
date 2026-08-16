import AutoExtractSection from './AutoExtractSection';
import HarmonySection from './HarmonySection';
import ImportExportSection from './ImportExportSection';
import PaletteManualEditor from './PaletteManualEditor';
import PaletteManagerSection from './PaletteManagerSection';
import PaletteVolumeViewer from './PaletteVolumeViewer';
import RampGeneratorSection from './RampGeneratorSection';
import ColorLabFooter from './ColorLabFooter';
import Icon from '../../icons/iconRegistry';
import Tooltip from '../../shared/ui/Tooltip';
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
  onReset: () => void;
  onApply: () => void;
  onImport: () => void;
  onExport: (format?: string) => void;
  onInsertGeneratedColors: (hexColors: string[]) => void;
  onGeneratorError: (message: string | null) => void;
}

const PLACEHOLDER = 'name-of-saved-palette';

/** Shared Color Lab body — compact sidebar or full floating layout. */
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
        previewColors={props.colors
          .filter((c) => c.valid)
          .map((c) => [c.r, c.g, c.b] as [number, number, number])}
      />

      {isSidebar ? (
        <AutoExtractSection
          compact
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
      ) : (
        <div className={cn('color-lab-grid')}>
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
          <ImportExportSection
            canExport={props.colors.length > 0}
            onImport={props.onImport}
            onExport={props.onExport}
          />
        </div>
      )}

      {!isSidebar && (
        <div className={cn('color-lab-grid')}>
          <RampGeneratorSection
            onInsert={props.onInsertGeneratedColors}
            onError={props.onGeneratorError}
          />
          <HarmonySection
            onInsert={props.onInsertGeneratedColors}
            onError={props.onGeneratorError}
          />
        </div>
      )}

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
        showSectionTitle={!isSidebar}
        selectedIndex={props.selectedColorIndex}
        onSelect={props.onSelectColor}
        onChange={props.onColorChange}
        onDelete={props.onDeleteColor}
        onAdd={props.onAddColor}
        onOpenPicker={props.onOpenPicker}
      />

      <PaletteVolumeViewer
        colors={props.colors}
        selectedIndex={props.selectedColorIndex}
        onSelectIndex={props.onSelectColor}
        compact={isSidebar}
      />

      {(props.error || props.successMessage) && (
        <>
          {props.error && <div className={cn('color-lab-error')}>{props.error}</div>}
          {props.successMessage && (
            <div className={cn('color-lab-success')}>{props.successMessage}</div>
          )}
        </>
      )}

      {isSidebar ? (
        <div className={cn('color-lab-utility-grid')}>
          <Tooltip label="Sort by brightness">
            <button
              type="button"
              onClick={props.onSort}
              className={cn('color-lab-button', 'color-lab-button-icon')}
              aria-label="Sort by brightness"
            >
              <Icon name="sort" width={16} height={16} />
            </button>
          </Tooltip>
          <Tooltip label="Auto interpolate">
            <button
              type="button"
              disabled
              className={cn('color-lab-button', 'color-lab-button-icon')}
              aria-label="Auto interpolate"
            >
              <Icon name="auto-interpolate" width={16} height={16} />
            </button>
          </Tooltip>
        </div>
      ) : (
        <ColorLabFooter
          cancelLabel="Reset"
          onSort={props.onSort}
          onCancel={props.onReset}
          onApply={props.onApply}
        />
      )}
    </div>
  );
}
