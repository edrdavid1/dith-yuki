import type { DockSide } from '../../types/panels';
import type { FilterInfo, FilterKind } from '../../types';
import type { EffectType } from '../../types/effects';
import { EFFECT_TO_FILTER_KIND } from '../../types/effects';
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
import styles from './EffectSettingsPanel.module.css';
import { bind } from '../../shared/ui/cn';
const cn = bind(styles);

export interface LayerWithFilters {
  id: number;
  name: string;
  filters: FilterInfo[];
}

export interface EffectSettingsPanelProps {
  selectedLayer: LayerWithFilters | null;
  onUpdateParams: (layerId: number, filterId: string, params: Record<string, unknown>) => void;
  onSelectEffect?: (effectType: EffectType) => void;
  onTitleBarMouseDown?: (e: React.MouseEvent) => void;
  dockSide?: DockSide;
  onMoveToSide?: (side: DockSide) => void;
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
    default:
      return null;
  }
}

const EFFECT_OPTIONS: { type: EffectType; label: string }[] = [
  { type: 'Dithering', label: 'Dithering' },
  { type: 'Glitching', label: 'Glitching' },
  { type: 'Curves', label: 'Curves' },
  { type: 'RGBChannels', label: 'RGB channels' },
  { type: 'Glow', label: 'Glow' },
  { type: 'CRT', label: 'CRT' },
  { type: 'Adjust', label: 'Adjust' },
];

/**
 * Thin effect settings switcher — editors live in `features/effects/editors/*`.
 */
export default function EffectSettingsPanel({
  selectedLayer,
  onUpdateParams,
  onSelectEffect,
  onTitleBarMouseDown,
  dockSide,
  onMoveToSide,
  targetLayerId = null,
  onExportPattern,
  onImportPattern,
}: EffectSettingsPanelProps) {
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
      <div className={cn('effect-settings-panel', 'effect-chooser-panel')}>
        <WindowTitlebar
          title="Effect"
          onMouseDown={onTitleBarMouseDown}
          dockSide={dockSide}
          onMoveToSide={onMoveToSide}
        />
        <div className={cn("effect-settings-scroll")}>
          <SimpleBar style={{ height: '100%' }}>
            <div className={cn("effect-chooser-list")} role="listbox" aria-label="Choose effect type">
              {EFFECT_OPTIONS.map((option) => (
                <button
                  key={option.type}
                  className={cn("effect-chooser-row")}
                  role="option"
                  aria-selected={false}
                  onClick={() => onSelectEffect?.(option.type)}
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
    const params = filter.params as unknown as Record<string, unknown>;
    switch (effectType) {
      case 'Dithering':
        return <DitherSettings params={params} onUpdate={handleUpdate} />;
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
        return <div className={cn("effect-settings-content")}>Unknown effect type</div>;
    }
  };

  return (
    <div className={cn("effect-settings-panel")}>
      <WindowTitlebar
        title={effectType || 'Dithering'}
        onMouseDown={onTitleBarMouseDown}
        dockSide={dockSide}
        onMoveToSide={onMoveToSide}
      />
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
