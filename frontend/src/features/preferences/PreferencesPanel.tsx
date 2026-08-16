import { useCallback, useState } from 'react';
import { moveAllPanelsToSide } from '../../shared/ipc/panels';
import { useShell } from '../../app/shell/ShellContext';
import {
  applyWorkspacePreset,
  captureWorkspacePreset,
  deleteWorkspacePreset,
  listWorkspacePresets,
  type WorkspacePreset,
} from '../panels/workspacePresets';
import styles from './PreferencesPanel.module.css';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...paramStyles });

/**
 * Application preferences body (chrome comes from PreferencesDialog).
 */
export default function PreferencesPanel() {
  const {
    leftSidebar,
    rightSidebar,
    leftSplitRatio,
    rightSplitRatio,
    setSidebarCollapsed,
    setSidebarWidth,
    setSplitRatio,
    resetSidebarWidths,
    autoExtractPalettes,
    setAutoExtractPalettes,
  } = useShell();

  const [presets, setPresets] = useState<WorkspacePreset[]>(() => listWorkspacePresets());
  const [saveName, setSaveName] = useState('');
  const [busy, setBusy] = useState(false);

  const refreshPresets = useCallback(() => {
    setPresets(listWorkspacePresets());
  }, []);

  const handleApplyPreset = useCallback(
    async (preset: WorkspacePreset) => {
      setBusy(true);
      try {
        await applyWorkspacePreset(preset, {
          setSidebarWidth,
          setSidebarCollapsed,
          setSplitRatio,
        });
      } catch (err) {
        console.error('Apply workspace preset failed:', err);
      } finally {
        setBusy(false);
      }
    },
    [setSidebarWidth, setSidebarCollapsed, setSplitRatio]
  );

  const handleSavePreset = useCallback(async () => {
    setBusy(true);
    try {
      await captureWorkspacePreset(saveName, {
        leftSidebar,
        rightSidebar,
        leftSplitRatio,
        rightSplitRatio,
      });
      setSaveName('');
      refreshPresets();
    } catch (err) {
      console.error('Save workspace preset failed:', err);
    } finally {
      setBusy(false);
    }
  }, [
    saveName,
    leftSidebar,
    rightSidebar,
    leftSplitRatio,
    rightSplitRatio,
    refreshPresets,
  ]);

  const handleDeletePreset = useCallback(
    (id: string) => {
      deleteWorkspacePreset(id);
      refreshPresets();
    },
    [refreshPresets]
  );

  return (
    <div className={cn('preferences-panel')}>
      <section className={cn('preferences-section')} aria-labelledby="prefs-layout-heading">
        <h2 id="prefs-layout-heading" className={cn('preferences-section-title')}>
          Layout
        </h2>

        <div className={cn('param-group')}>
          <span className={cn('preferences-label')}>Panel stack</span>
          <div className={cn('preferences-btn-row')}>
            <button
              type="button"
              className={cn('preferences-button')}
              onClick={() => {
                void moveAllPanelsToSide('left').catch((err) =>
                  console.error('Move all to left failed:', err)
                );
              }}
            >
              Move all panels to left
            </button>
            <button
              type="button"
              className={cn('preferences-button')}
              onClick={() => {
                void moveAllPanelsToSide('right').catch((err) =>
                  console.error('Move all to right failed:', err)
                );
              }}
            >
              Move all panels to right
            </button>
          </div>
        </div>

        <div className={cn('param-group')}>
          <label className={cn('preferences-checkbox-row')}>
            <input
              type="checkbox"
              checked={leftSidebar.collapsed}
              onChange={(e) => setSidebarCollapsed('left', e.target.checked)}
            />
            <span>Collapse left sidebar</span>
          </label>
        </div>

        <div className={cn('param-group')}>
          <label className={cn('preferences-checkbox-row')}>
            <input
              type="checkbox"
              checked={rightSidebar.collapsed}
              onChange={(e) => setSidebarCollapsed('right', e.target.checked)}
            />
            <span>Collapse right sidebar</span>
          </label>
        </div>

        <div className={cn('param-group')}>
          <button type="button" className={cn('preferences-button')} onClick={resetSidebarWidths}>
            Reset sidebar widths
          </button>
        </div>
      </section>

      <section className={cn('preferences-section')} aria-labelledby="prefs-workspace-heading">
        <h2 id="prefs-workspace-heading" className={cn('preferences-section-title')}>
          Workspace presets
        </h2>

        <div className={cn('preferences-btn-row')}>
          {presets.map((preset) => (
            <div key={preset.id} className={cn('preferences-preset-row')}>
              <button
                type="button"
                className={cn('preferences-button')}
                disabled={busy}
                onClick={() => void handleApplyPreset(preset)}
              >
                {preset.name}
              </button>
              {!preset.builtin && (
                <button
                  type="button"
                  className={cn('preferences-button', 'preferences-button-danger')}
                  disabled={busy}
                  onClick={() => handleDeletePreset(preset.id)}
                  aria-label={`Delete ${preset.name}`}
                >
                  Delete
                </button>
              )}
            </div>
          ))}
        </div>

        <div className={cn('param-group')}>
          <label className={cn('preferences-label')} htmlFor="workspace-preset-name">
            Save current layout
          </label>
          <div className={cn('preferences-save-row')}>
            <input
              id="workspace-preset-name"
              className={cn('preferences-input')}
              value={saveName}
              onChange={(e) => setSaveName(e.target.value)}
              placeholder="My layout"
              disabled={busy}
            />
            <button
              type="button"
              className={cn('preferences-button')}
              disabled={busy || !saveName.trim()}
              onClick={() => void handleSavePreset()}
            >
              Save
            </button>
          </div>
        </div>
      </section>

      <section className={cn('preferences-section')} aria-labelledby="prefs-color-heading">
        <h2 id="prefs-color-heading" className={cn('preferences-section-title')}>
          Color / Palettes
        </h2>

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
      </section>
    </div>
  );
}
