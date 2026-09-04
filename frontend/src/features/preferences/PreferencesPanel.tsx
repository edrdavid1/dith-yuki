import { useCallback, useEffect, useState } from 'react';
import SimpleBar from 'simplebar-react';
import { useShell } from '../../app/shell/ShellContext';
import {
  PREVIEW_BACKGROUNDS,
  previewBackgroundStyle,
} from '../preview/previewBackground';
import {
  applyWorkspacePreset,
  builtinWorkspacePresets,
  type WorkspacePreset,
} from '../panels/workspacePresets';
import {
  eventToChord,
  formatChords,
  SHORTCUT_IDS,
  SHORTCUT_LABELS,
} from '../shortcuts/bindings';
import { useShortcuts } from '../shortcuts/ShortcutsContext';
import styles from './PreferencesPanel.module.css';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...paramStyles });

const LAYOUT_PRESETS = builtinWorkspacePresets();

/**
 * Application preferences body (chrome comes from PreferencesDialog).
 */
export default function PreferencesPanel() {
  const {
    setSidebarCollapsed,
    setSidebarWidth,
    setSplitRatio,
    autoExtractPalettes,
    setAutoExtractPalettes,
    previewBackground,
    setPreviewBackground,
  } = useShell();
  const { bindings, capturing, setCapturing, setBinding, resetDefaults } = useShortcuts();

  const [busy, setBusy] = useState(false);
  const [activePresetId, setActivePresetId] = useState<string | null>(null);

  useEffect(() => {
    if (!capturing) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (['Meta', 'Control', 'Alt', 'Shift'].includes(e.key)) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        setCapturing(null);
        return;
      }
      setBinding(capturing, [eventToChord(e)]);
      setCapturing(null);
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [capturing, setBinding, setCapturing]);

  const handleApplyPreset = useCallback(
    async (preset: WorkspacePreset) => {
      setBusy(true);
      try {
        await applyWorkspacePreset(preset, {
          setSidebarWidth,
          setSidebarCollapsed,
          setSplitRatio,
        });
        setActivePresetId(preset.id);
      } catch (err) {
        console.error('Apply workspace preset failed:', err);
      } finally {
        setBusy(false);
      }
    },
    [setSidebarWidth, setSidebarCollapsed, setSplitRatio]
  );

  return (
    <div className={cn('preferences-panel')}>
      <SimpleBar className={cn('preferences-scroll')} style={{ height: '100%' }}>
        <div className={cn('preferences-body')}>
      <details className={cn('preferences-section')} open>
        <summary id="prefs-layout-heading" className={cn('preferences-section-title')}>
          Layout
        </summary>

        <p className={cn('preferences-hint')}>
          Starting layouts: pick which panel sits on the left, the rest go to the
          right. You can still rearrange everything yourself — drag panels between
          sides or into floating windows.
        </p>

        <div className={cn('preferences-btn-row')} role="group" aria-label="Workspace layout">
          {LAYOUT_PRESETS.map((preset) => (
            <button
              key={preset.id}
              type="button"
              className={cn(
                'preferences-button',
                activePresetId === preset.id && 'preferences-button-active'
              )}
              disabled={busy}
              aria-pressed={activePresetId === preset.id}
              onClick={() => void handleApplyPreset(preset)}
            >
              {preset.name}
            </button>
          ))}
        </div>
      </details>

      <details className={cn('preferences-section')}>
        <summary id="prefs-color-heading" className={cn('preferences-section-title')}>
          Color / Palettes
        </summary>

        <div className={cn('param-group')}>
          <label className={cn('preferences-checkbox-row')}>
            <input
              type="checkbox"
              checked={autoExtractPalettes}
              onChange={(e) => setAutoExtractPalettes(e.target.checked)}
            />
            <span>Automatically extract palette when adding an image</span>
          </label>
        </div>
      </details>

      <details className={cn('preferences-section')}>
        <summary id="prefs-preview-heading" className={cn('preferences-section-title')}>
          Preview
        </summary>
        <p className={cn('preferences-hint')}>
          Fill behind the image in the preview canvas.
        </p>
        <div className={cn('preferences-swatch-row')} role="group" aria-label="Preview background">
          {PREVIEW_BACKGROUNDS.map((preset) => {
            const selected = previewBackground === preset.id;
            return (
              <button
                key={preset.id}
                type="button"
                className={cn(
                  'preferences-swatch',
                  selected && 'preferences-swatch-active'
                )}
                style={previewBackgroundStyle(preset.id)}
                aria-label={preset.label}
                aria-pressed={selected}
                title={preset.label}
                onClick={() => setPreviewBackground(preset.id)}
              />
            );
          })}
        </div>
      </details>

      <details
        className={cn('preferences-section')}
        onToggle={(e) => {
          if (!(e.currentTarget as HTMLDetailsElement).open) setCapturing(null);
        }}
      >
        <summary id="prefs-keys-heading" className={cn('preferences-section-title')}>
          Keyboard shortcuts
        </summary>
        <p className={cn('preferences-hint')}>
          Defaults match Photoshop. Click a shortcut, then press the new keys.
          Escape cancels. Conflicts are taken from the other command.
        </p>
        <div className={cn('preferences-shortcut-list')} role="list">
          {SHORTCUT_IDS.map((id) => {
            const chords = bindings[id];
            const label = chords.length > 0 ? formatChords(chords) : 'None';
            const active = capturing === id;
            return (
              <div key={id} className={cn('preferences-shortcut-row')} role="listitem">
                <span className={cn('preferences-shortcut-name')}>{SHORTCUT_LABELS[id]}</span>
                <button
                  type="button"
                  className={cn(
                    'preferences-shortcut-bind',
                    active && 'preferences-shortcut-bind-active'
                  )}
                  aria-label={`Set shortcut for ${SHORTCUT_LABELS[id]}`}
                  onClick={() => setCapturing(active ? null : id)}
                >
                  {active ? 'Press keys…' : label}
                </button>
              </div>
            );
          })}
        </div>
        <button
          type="button"
          className={cn('preferences-button')}
          onClick={resetDefaults}
        >
          Restore Photoshop defaults
        </button>
      </details>
        </div>
      </SimpleBar>
    </div>
  );
}
