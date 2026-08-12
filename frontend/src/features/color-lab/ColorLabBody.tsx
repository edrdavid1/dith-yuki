import DropdownMenu from '../../components/common/DropdownMenu';
import AutoExtractSection from './AutoExtractSection';
import BuiltinPresetsSection from './BuiltinPresetsSection';
import HarmonySection from './HarmonySection';
import ImportExportSection from './ImportExportSection';
import PaletteManualEditor from './PaletteManualEditor';
import RampGeneratorSection from './RampGeneratorSection';
import ColorLabFooter from './ColorLabFooter';
import type { ColorEntry, ExtractMethod } from './types';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

export type ColorLabVariant = 'sidebar' | 'full';

export interface PaletteOption {
  id: number;
  name: string;
}

export interface ColorLabBodyProps {
  variant: ColorLabVariant;
  name: string;
  onNameChange: (name: string) => void;
  paletteOptions: PaletteOption[];
  selectedPaletteId: number | null;
  onSelectPalette: (paletteId: number) => void;
  extractMethod: ExtractMethod;
  extractCount: number;
  onMethodChange: (method: ExtractMethod) => void;
  onCountChange: (count: number) => void;
  onExtractRaw: () => void;
  onExtractActual: () => void;
  colors: ColorEntry[];
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
  onImportBuiltin: (id: string) => void;
  onInsertGeneratedColors: (hexColors: string[]) => void;
  onGeneratorError: (message: string | null) => void;
}

const PLACEHOLDER = 'name-of-saved-palette';

/** Shared Color Lab body — compact sidebar or full floating layout. */
export default function ColorLabBody(props: ColorLabBodyProps) {
  const isSidebar = props.variant === 'sidebar';

  const paletteOptions = props.paletteOptions.map((p) => ({
    value: String(p.id),
    label: p.name,
  }));

  const dropdownValue =
    props.selectedPaletteId !== null ? String(props.selectedPaletteId) : PLACEHOLDER;

  const dropdownOptions =
    props.selectedPaletteId === null
      ? [{ value: PLACEHOLDER, label: PLACEHOLDER, disabled: true }, ...paletteOptions]
      : paletteOptions.length > 0
        ? paletteOptions
        : [{ value: PLACEHOLDER, label: PLACEHOLDER, disabled: true }];

  return (
    <div className={cn('color-lab-body', isSidebar && 'color-lab-body-sidebar')}>
      {isSidebar ? (
        <AutoExtractSection
          compact
          extractMethod={props.extractMethod}
          extractCount={props.extractCount}
          onMethodChange={props.onMethodChange}
          onCountChange={props.onCountChange}
          onExtractRaw={props.onExtractRaw}
          onExtractActual={props.onExtractActual}
        />
      ) : (
        <div className={cn('color-lab-grid')}>
          <AutoExtractSection
            extractMethod={props.extractMethod}
            extractCount={props.extractCount}
            onMethodChange={props.onMethodChange}
            onCountChange={props.onCountChange}
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

      <BuiltinPresetsSection onImportBuiltin={props.onImportBuiltin} />

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

      {isSidebar ? (
        <DropdownMenu
          value={dropdownValue}
          options={dropdownOptions}
          onSelect={(v) => {
            if (v === PLACEHOLDER) return;
            const id = Number(v);
            if (!Number.isFinite(id)) return;
            props.onSelectPalette(id);
          }}
        />
      ) : (
        <input
          type="text"
          className={cn('color-lab-name-input')}
          value={props.name}
          onChange={(e) => props.onNameChange(e.target.value)}
          placeholder={PLACEHOLDER}
          aria-label="Palette name"
        />
      )}

      <PaletteManualEditor
        colors={props.colors}
        canAddColor={props.canAddColor}
        compact={isSidebar}
        showSectionTitle={!isSidebar}
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

      {isSidebar ? (
        <div className={cn('color-lab-utility-grid')}>
          <button type="button" onClick={props.onSort} className={cn('color-lab-button')}>
            <img src="/icons/sort-icon.svg" width={16} height={16} alt="" />
            Sort by brightness
          </button>
          <button type="button" disabled className={cn('color-lab-button')}>
            <img src="/icons/auto-interpolate-icon.svg" width={16} height={16} alt="" />
            Auto interpolate
          </button>
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
