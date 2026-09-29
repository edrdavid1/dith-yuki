import { useCallback, useEffect, useState } from 'react';
import SimpleBar from 'simplebar-react';
import { useShell } from '../../app/shell/ShellContext';
import { isMacOS } from '../../lib/platform';
import {
  getAppIcon,
  listAppIcons,
  setAppIcon,
  type AppIconVariant,
} from '../../shared/ipc/appIcon';
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
    appIconId,
    setAppIconId,
    hapticFeedback,
    setHapticFeedback,
  } = useShell();
  const { bindings, capturing, setCapturing, setBinding, resetDefaults } = useShortcuts();
  const showAppIcon = isMacOS();
  const [iconVariants, setIconVariants] = useState<AppIconVariant[]>([]);
  const [iconWarning, setIconWarning] = useState<string | null>(null);
  const [iconBusy, setIconBusy] = useState(false);

  useEffect(() => {
    if (!showAppIcon) return;
    let cancelled = false;
    void Promise.all([listAppIcons(), getAppIcon()])
      .then(([variants, state]) => {
        if (cancelled) return;
        setIconVariants(variants);
        setAppIconId(state.id);
      })
      .catch(() => {
        if (!cancelled) setIconVariants([]);
      });
    return () => {
      cancelled = true;
    };
  }, [showAppIcon, setAppIconId]);

  const handleSelectAppIcon = useCallback(
    async (id: string) => {
      if (iconBusy || id === appIconId) return;
      setIconBusy(true);
      try {
        const result = await setAppIcon(id);
        setAppIconId(result.id);
        setIconWarning(result.warning);
      } catch (err) {
        console.warn('set_app_icon failed', err);
      } finally {
        setIconBusy(false);
      }
    },
    [appIconId, iconBusy, setAppIconId]
  );

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

      <details className={cn('preferences-section')} open>
        <summary id="prefs-interface-heading" className={cn('preferences-section-title')}>
          Interface
        </summary>
        <div className={cn('param-group')}>
          <label className={cn('preferences-checkbox-row')}>
            <input
              type="checkbox"
              checked={hapticFeedback}
              onChange={(e) => setHapticFeedback(e.target.checked)}
            />
            <span>Tactile feedback on sliders</span>
          </label>
          <p className={cn('preferences-hint')}>
            {isMacOS()
              ? 'Uses the Force Touch trackpad Taptic Engine while dragging sliders. Has no effect with a mouse or when system haptic feedback is off.'
              : 'macOS only (Force Touch trackpad). Has no effect on this platform.'}
          </p>
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

        {showAppIcon && iconVariants.length > 0 ? (
          <>
            <p className={cn('preferences-label', 'preferences-label-spaced')}>App icon</p>
            <div
              className={cn('preferences-swatch-row')}
              role="group"
              aria-label="App icon"
            >
              {iconVariants.map((variant) => {
                const selected = appIconId === variant.id;
                return (
                  <button
                    key={variant.id}
                    type="button"
                    className={cn(
                      'preferences-swatch',
                      'preferences-icon-swatch',
                      selected && 'preferences-swatch-active'
                    )}
                    style={{ backgroundImage: `url(${variant.previewSrc})` }}
                    aria-label={variant.label}
                    aria-pressed={selected}
                    title={variant.todoDesign ? `${variant.label} (placeholder)` : variant.label}
                    disabled={iconBusy}
                    onClick={() => void handleSelectAppIcon(variant.id)}
                  />
                );
              })}
            </div>
            {iconWarning ? (
              <p className={cn('preferences-hint')} role="status">
                {iconWarning}
              </p>
            ) : null}
          </>
        ) : null}
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
