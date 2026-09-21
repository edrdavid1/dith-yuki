import { useEffect } from 'react';
import SimpleBar from 'simplebar-react';
import { useShell } from '../../app/shell/ShellContext';
import {
  PREVIEW_BACKGROUNDS,
  previewBackgroundStyle,
} from '../preview/previewBackground';
import {
  WELCOME_BACKGROUNDS,
  welcomeBackgroundStyle,
} from '../preview/welcomeBackground';
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

/**
 * Application preferences body (chrome comes from PreferencesDialog).
 */
export default function PreferencesPanel() {
  const {
    autoExtractPalettes,
    setAutoExtractPalettes,
    previewBackground,
    setPreviewBackground,
    welcomeBackground,
    setWelcomeBackground,
    hideRecentList,
    setHideRecentList,
  } = useShell();
  const { bindings, capturing, setCapturing, setBinding, resetDefaults } = useShortcuts();

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

  return (
    <div className={cn('preferences-panel')}>
      <SimpleBar className={cn('preferences-scroll')} style={{ height: '100%' }}>
        <div className={cn('preferences-body')}>
      <details className={cn('preferences-section')} open>
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
        <summary id="prefs-theme-heading" className={cn('preferences-section-title')}>
          Theme
        </summary>

        <p className={cn('preferences-label')}>Preview background</p>
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

        <p className={cn('preferences-label', 'preferences-label-spaced')}>
          Welcome background
        </p>
        <div
          className={cn('preferences-thumb-row')}
          role="group"
          aria-label="Welcome background"
        >
          {WELCOME_BACKGROUNDS.map((preset) => {
            const selected = welcomeBackground === preset.id;
            return (
              <button
                key={preset.id}
                type="button"
                className={cn(
                  'preferences-thumb',
                  selected && 'preferences-thumb-active'
                )}
                style={welcomeBackgroundStyle(preset.id)}
                aria-label={preset.label}
                aria-pressed={selected}
                onClick={() => setWelcomeBackground(preset.id)}
              />
            );
          })}
        </div>

        <div className={cn('param-group', 'preferences-label-spaced')}>
          <label className={cn('preferences-checkbox-row')}>
            <input
              type="checkbox"
              checked={hideRecentList}
              onChange={(e) => setHideRecentList(e.target.checked)}
            />
            <span>Hide recent files list on welcome screen</span>
          </label>
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
