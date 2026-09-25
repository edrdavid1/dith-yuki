import type { DockSide } from '../../types/panels';
import type { FilterInfo, FilterKind } from '../../types';
import { EFFECT_TO_FILTER_KIND, isDitheringAlgorithmId, type EffectType } from '../../types/effects';
import Icon from '../../icons/iconRegistry';
import Tooltip from '../../shared/ui/Tooltip';
import SimpleBar from 'simplebar-react';
import WindowTitlebar from '../../shared/ui/WindowTitlebar';
import DitherSettings from './editors/DitherSettings';
import GlitchSettings from './editors/GlitchSettings';
import CurvesSettings from './editors/CurvesSettings';
import RGBSettings from './editors/RGBSettings';
import GlowSettings from './editors/GlowSettings';
import CrtSettings from './editors/CrtSettings';
import AdjustSettings from './editors/AdjustSettings';
import AsciiSettings from './editors/AsciiSettings';
import AlgorithmSettingsPanel from './AlgorithmSettingsPanel';
import { useAlgorithmCatalog } from './hooks/useAlgorithmCatalog';
import { EFFECT_PRESETS } from './effectPresets';
import { unwrapFilterParams } from '../../shared/unwrapFilterParams';
import type { AlgorithmInfo, EffectCategory } from '../../shared/ipc/registry';
import styles from './EffectSettingsPanel.module.css';
import { bind } from '../../shared/ui/cn';
const cn = bind(styles);

/**
 * Fire on pointerdown so the first click works even when the main window was
 * unfocused (e.g. Preview flex-popout had focus — macOS/WKWebView focus-steal).
 */
function chooseOnPointerDown(action: () => void) {
  return (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    action();
  };
}

export interface LayerWithFilters {
  id: number;
  name: string;
  filters: FilterInfo[];
}

export interface EffectSettingsPanelProps {
  selectedLayer: LayerWithFilters | null;
  onUpdateParams: (layerId: number, filterId: string, params: Record<string, unknown>) => void;
  onSelectEffect?: (effectType: EffectType) => void;
  onSelectAlgorithm?: (algorithmId: string) => void;
  onSelectPreset?: (presetId: string) => void;
  onTitleBarMouseDown?: (e: React.MouseEvent) => void;
  dockSide?: DockSide;
  onMoveToSide?: (side: DockSide) => void;
  /** When true, omit WindowTitlebar — FlexLayout tab strip is the chrome. */
  hideChrome?: boolean;
  onPopOut?: () => void;
  onDockBack?: () => void;
  /** Leaf layer to export/import a `.dyuki` pattern against. */
  targetLayerId?: number | null;
  onExportPattern?: () => void;
  onImportPattern?: () => void;
}

function filterKindToEffectType(kind: FilterKind): EffectType | null {
  for (const [effectType, filterKind] of Object.entries(EFFECT_TO_FILTER_KIND)) {
    if (filterKind === kind) return effectType as EffectType;
  }
  if (kind === 'Dither') return 'Dithering';
  return null;
}

function EffectIcon({ type }: { type: EffectType }) {
  switch (type) {
    case 'Dithering':
      return <Icon name="effect.dithering" width={20} height={20} />;
    case 'Glitching':
      return <Icon name="effect.glitching" width={20} height={20} />;
    case 'Curves':
      return <Icon name="effect.curves" width={20} height={20} />;
    case 'RGBChannels':
      return <Icon name="effect.rgb" width={20} height={20} />;
    case 'Glow':
      return <Icon name="effect.glow" width={20} height={20} />;
    case 'CRT':
      return <Icon name="effect.crt" width={20} height={20} />;
    case 'Adjust':
      return <Icon name="effect.adjust" width={20} height={20} />;
    case 'Ascii':
      return <Icon name="effect.ascii" width={20} height={20} />;
    default:
      return null;
  }
}

function CategoryIcon({ category }: { category: EffectCategory }) {
  switch (category) {
    case 'dithering':
    case 'palette':
      return <Icon name="effect.dithering" width={20} height={20} />;
    case 'glitch':
      return <Icon name="effect.glitching" width={20} height={20} />;
    case 'color_adjust':
      return <Icon name="effect.adjust" width={20} height={20} />;
    case 'stylize':
      return <Icon name="effect.crt" width={20} height={20} />;
    case 'ascii':
      return <Icon name="effect.ascii" width={20} height={20} />;
    default:
      return null;
  }
}

/** Levels is not in the Registry; keep it as a static chooser row. */
const LEGACY_CHOOSER: { type: EffectType; label: string }[] = [
  { type: 'RGBChannels', label: 'RGB channels' },
];

/** Non-dither registry ids that still map to the legacy EffectType chooser/editors. */
const ALGO_TO_EFFECT: Record<string, { type: EffectType; label: string }> = {
  glitch: { type: 'Glitching', label: 'Glitching' },
  curves: { type: 'Curves', label: 'Curves' },
  glow: { type: 'Glow', label: 'Glow' },
  crt: { type: 'CRT', label: 'CRT' },
  adjust: { type: 'Adjust', label: 'Adjust' },
  ascii: { type: 'Ascii', label: 'ASCII' },
};

/**
 * Thin effect settings switcher — editors live in `features/effects/editors/*`.
 */
export default function EffectSettingsPanel({
  selectedLayer,
  onUpdateParams,
  onSelectEffect,
  onSelectAlgorithm,
  onSelectPreset,
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
  hideChrome = false,
  onPopOut,
  onDockBack,
  targetLayerId = null,
  onExportPattern,
  onImportPattern,
}: EffectSettingsPanelProps) {
  const catalog = useAlgorithmCatalog();
  const canUsePattern = targetLayerId != null;
  const patternActions = (
    <div className={cn('pattern-actions')}>
      <Tooltip label="Export as pattern">
        <button
          type="button"
          className={cn('pattern-action-btn')}
          disabled={!canUsePattern}
          onClick={() => onExportPattern?.()}
          aria-label="Export as pattern"
        >
          <Icon name="export" width={16} height={16} />
        </button>
      </Tooltip>
      <Tooltip label="Import pattern">
        <button
          type="button"
          className={cn('pattern-action-btn')}
          disabled={!canUsePattern}
          onClick={() => onImportPattern?.()}
          aria-label="Import pattern"
        >
          <Icon name="import" width={16} height={16} />
        </button>
      </Tooltip>
    </div>
  );

  if (!selectedLayer || selectedLayer.filters.length === 0) {
    return (
      <div
        className={cn('effect-settings-panel', 'effect-chooser-panel')}
        data-dock-window="effect"
        data-dock-side={dockSide}
      >
        {!hideChrome && (
          <WindowTitlebar
            title="Effect"
            onMouseDown={onTitleBarMouseDown}
            dockSide={dockSide}
            onMoveToSide={onMoveToSide}
            onPopOut={onPopOut}
            onDockBack={onDockBack}
          />
        )}
        <div className={cn("effect-settings-scroll")}>
          <SimpleBar style={{ height: '100%' }}>
            <div className={cn("effect-chooser-list")} role="listbox" aria-label="Choose effect type">
              {catalog == null && (
                <div className={cn('effect-chooser-row-label')} role="status">Loading…</div>
              )}
              {catalog != null && catalog.some((a) => a.category === 'dithering') && (
                <button
                  type="button"
                  className={cn("effect-chooser-row")}
                  role="option"
                  aria-selected={false}
                  onPointerDown={chooseOnPointerDown(() => onSelectEffect?.('Dithering'))}
                  onClick={(e) => e.preventDefault()}
                >
                  <div className={cn("effect-chooser-row-icon")}>
                    <EffectIcon type="Dithering" />
                  </div>
                  <div className={cn("effect-chooser-row-label")}>
                    <span>Dithering</span>
                  </div>
                </button>
              )}
              {catalog != null && catalog.some((a) => a.category === 'ascii') && (
                <button
                  type="button"
                  className={cn("effect-chooser-row")}
                  role="option"
                  aria-selected={false}
                  onPointerDown={chooseOnPointerDown(() => onSelectEffect?.('Ascii'))}
                  onClick={(e) => e.preventDefault()}
                >
                  <div className={cn("effect-chooser-row-icon")}>
                    <EffectIcon type="Ascii" />
                  </div>
                  <div className={cn("effect-chooser-row-label")}>
                    <span>ASCII</span>
                  </div>
                </button>
              )}
              {catalog
                ?.filter(
                  (algo: AlgorithmInfo) =>
                    algo.category !== 'dithering' && algo.category !== 'ascii'
                )
                .map((algo) => {
                const mapped = ALGO_TO_EFFECT[algo.id];
                return (
                  <button
                    key={algo.id}
                    className={cn("effect-chooser-row")}
                    role="option"
                    aria-selected={false}
                    onPointerDown={chooseOnPointerDown(() =>
                      mapped
                        ? onSelectEffect?.(mapped.type)
                        : onSelectAlgorithm?.(algo.id)
                    )}
                    onClick={(e) => e.preventDefault()}
                    type="button"
                  >
                    <div className={cn("effect-chooser-row-icon")}>
                      {mapped ? (
                        <EffectIcon type={mapped.type} />
                      ) : (
                        <CategoryIcon category={algo.category} />
                      )}
                    </div>
                    <div className={cn("effect-chooser-row-label")}>
                      <span>{mapped?.label ?? algo.display_name}</span>
                    </div>
                  </button>
                );
              })}
              {LEGACY_CHOOSER.map((option) => (
                <button
                  key={option.type}
                  className={cn("effect-chooser-row")}
                  role="option"
                  aria-selected={false}
                  onPointerDown={chooseOnPointerDown(() => onSelectEffect?.(option.type))}
                  onClick={(e) => e.preventDefault()}
                  type="button"
                >
                  <div className={cn("effect-chooser-row-icon")}>
                    <EffectIcon type={option.type} />
                  </div>
                  <div className={cn("effect-chooser-row-label")}>
                    <span>{option.label}</span>
                  </div>
                </button>
              ))}
              {EFFECT_PRESETS.map((preset) => (
                <button
                  key={preset.id}
                  className={cn('effect-chooser-row')}
                  role="option"
                  aria-selected={false}
                  onPointerDown={chooseOnPointerDown(() => onSelectPreset?.(preset.id))}
                  onClick={(e) => e.preventDefault()}
                  type="button"
                  title={preset.hint}
                >
                  <div className={cn('effect-chooser-row-icon')}>
                    <EffectIcon type="Dithering" />
                  </div>
                  <div className={cn('effect-chooser-row-label')}>
                    <span>{preset.label}</span>
                  </div>
                </button>
              ))}
            </div>
            {patternActions}
          </SimpleBar>
        </div>
      </div>
    );
  }

  const filter = selectedLayer.filters[0];
  const effectType = filterKindToEffectType(filter.kind);

  const handleUpdate = (params: Record<string, unknown>) => {
    onUpdateParams(selectedLayer.id, filter.id, params);
  };

  const renderSettings = () => {
    const params = unwrapFilterParams(filter.params as unknown as Record<string, unknown>);
    const dithering =
      effectType === 'Dithering' ||
      filter.kind === 'DitherV2' ||
      filter.kind === 'Dither' ||
      (filter.algorithm_id != null && isDitheringAlgorithmId(filter.algorithm_id));
    if (dithering) {
      return <DitherSettings params={params} onUpdate={handleUpdate} />;
    }

    // Prefer dedicated editors over AlgorithmSettingsPanel. Registry migration
    // stamps algorithm_id on legacy effects (curves, glitch, …); Curves has an
    // empty param_schema, so the generic panel would render blank.
    const dedicated: EffectType | null =
      effectType ??
      (filter.algorithm_id != null ? (ALGO_TO_EFFECT[filter.algorithm_id]?.type ?? null) : null);

    switch (dedicated) {
      case 'Ascii':
        return <AsciiSettings params={params} onUpdate={handleUpdate} />;
      case 'Glitching':
        return <GlitchSettings params={params} onUpdate={handleUpdate} />;
      case 'Curves':
        return <CurvesSettings params={params} onUpdate={handleUpdate} />;
      case 'RGBChannels':
        return <RGBSettings params={params} onUpdate={handleUpdate} />;
      case 'Glow':
        return <GlowSettings params={params} onUpdate={handleUpdate} />;
      case 'CRT':
        return <CrtSettings params={params} onUpdate={handleUpdate} />;
      case 'Adjust':
        return <AdjustSettings params={params} onUpdate={handleUpdate} />;
      default:
        break;
    }

    if (filter.algorithm_id) {
      return (
        <AlgorithmSettingsPanel
          algorithmId={filter.algorithm_id}
          values={params}
          onChange={handleUpdate}
        />
      );
    }

    return <div className={cn("effect-settings-content")}>Unknown effect type</div>;
  };

  return (
    <div
      className={cn("effect-settings-panel")}
      data-dock-window="effect"
      data-dock-side={dockSide}
    >
      {!hideChrome && (
        <WindowTitlebar
          title={
            effectType === 'Dithering' ||
            (filter.algorithm_id != null && isDitheringAlgorithmId(filter.algorithm_id))
              ? 'Dithering'
              : effectType === 'Ascii' ||
                  filter.kind === 'Ascii' ||
                  filter.algorithm_id === 'ascii'
                ? 'ASCII'
                : (filter.algorithm_id ?? effectType ?? 'Dithering')
          }
          onMouseDown={onTitleBarMouseDown}
          dockSide={dockSide}
          onMoveToSide={onMoveToSide}
          onPopOut={onPopOut}
          onDockBack={onDockBack}
        />
      )}
      <div className={cn("effect-settings-scroll")}>
        <SimpleBar style={{ height: '100%' }}>
          <div className={cn("effect-settings-body")}>
            {renderSettings()}
            {patternActions}
          </div>
        </SimpleBar>
      </div>
    </div>
  );
}
