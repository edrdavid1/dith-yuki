import { useCallback, useEffect, useMemo, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import DropdownMenu from '../../components/common/DropdownMenu';
import Tooltip from '../../shared/ui/Tooltip';
import {
  proofGetConfig,
  proofImportProfile,
  proofListProfiles,
  proofSetConfig,
  softProofChipLabel,
  type ProofProfileInfo,
  type SoftProofConfig,
  type SoftProofIntent,
} from '../../shared/ipc/proof';
import { onDocumentChanged } from '../../shared/ipc/events';
import styles from './SoftProofControls.module.css';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import buttonStyles from '../color-lab/ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...paramStyles, ...buttonStyles });

const DEFAULT_SOFT_PROOF: SoftProofConfig = {
  enabled: false,
  profile_id: 'builtin:fogra51',
  intent: 'relative',
  bpc: true,
};

const INTENT_OPTIONS = [
  { value: 'relative', label: 'Relative' },
  { value: 'perceptual', label: 'Perceptual' },
  { value: 'absolute', label: 'Absolute' },
];

export interface SoftProofControlsProps {
  docId: number | null;
  /** Compact footer strip (Preview) vs stacked panel (Color Lab). */
  layout?: 'footer' | 'panel';
  /** Show Strict-palette warning when soft proof is on. */
  showStrictWarn?: boolean;
}

export default function SoftProofControls({
  docId,
  layout = 'footer',
  showStrictWarn = false,
}: SoftProofControlsProps) {
  const [softProof, setSoftProof] = useState<SoftProofConfig>(DEFAULT_SOFT_PROOF);
  const [proofProfiles, setProofProfiles] = useState<ProofProfileInfo[]>([]);
  const [settingsOpen, setSettingsOpen] = useState(layout === 'panel');
  const [loadError, setLoadError] = useState<string | null>(null);
  const [missingProfileToast, setMissingProfileToast] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (docId == null) {
      setSoftProof(DEFAULT_SOFT_PROOF);
      return;
    }
    try {
      const [cfg, profiles] = await Promise.all([
        proofGetConfig(docId),
        proofListProfiles(),
      ]);
      setSoftProof(cfg);
      setProofProfiles(profiles);
      setLoadError(profiles.length === 0 ? 'No ICC profiles available' : null);
      const missing =
        cfg.enabled &&
        cfg.profile_id &&
        !profiles.some((p) => p.id === cfg.profile_id);
      if (missing) {
        const name = cfg.profile_display_name || cfg.profile_id;
        setMissingProfileToast(`Profile ${name} not found — import`);
      } else {
        setMissingProfileToast(null);
      }
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    }
  }, [docId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (docId == null) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void onDocumentChanged((event) => {
      if (cancelled) return;
      if (event.payload.doc_id != null && event.payload.doc_id !== docId) return;
      if (
        event.payload.kind === 'soft_proof_changed' ||
        event.payload.kind === 'project_opened' ||
        event.payload.kind === 'document_activated'
      ) {
        void refresh();
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [docId, refresh]);

  const applySoftProof = useCallback(
    async (next: SoftProofConfig) => {
      if (docId == null) return;
      const t0 = performance.now();
      performance.mark('soft_proof_click');
      try {
        const saved = await proofSetConfig(docId, next);
        setSoftProof(saved);
        setLoadError(null);
        performance.mark('soft_proof_config_applied');
        performance.measure('soft_proof_click_to_config', 'soft_proof_click', 'soft_proof_config_applied');
        console.debug(
          `[soft_proof] click→config ${((performance.now() - t0)).toFixed(1)}ms enabled=${saved.enabled}`,
        );
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setLoadError(msg);
        if (msg.toLowerCase().includes('not found')) {
          setMissingProfileToast(msg);
        }
      }
    },
    [docId],
  );

  const importProofProfile = useCallback(async () => {
    if (docId == null) return;
    const selected = await open({
      filters: [{ name: 'ICC profiles', extensions: ['icc', 'icm'] }],
      multiple: false,
    });
    if (!selected || typeof selected !== 'string') return;
    try {
      const info = await proofImportProfile(selected);
      const profiles = await proofListProfiles();
      setProofProfiles(profiles);
      await applySoftProof({ ...softProof, profile_id: info.id, enabled: true });
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    }
  }, [applySoftProof, docId, softProof]);

  const disabled = docId == null;

  const profileOptions = useMemo(() => {
    if (proofProfiles.length === 0) {
      return [{ value: '', label: 'No profiles loaded', disabled: true }];
    }
    return proofProfiles.map((p) => ({
      value: p.id,
      label: p.name,
      group: p.builtin ? 'Built-in' : 'Imported',
    }));
  }, [proofProfiles]);

  const settings = (
    <div
      className={cn(layout === 'footer' ? 'sp-panel-popover' : 'sp-panel')}
      role={layout === 'footer' ? 'dialog' : 'group'}
      aria-label="Soft proof settings"
    >
      <DropdownMenu
        label="Profile"
        value={softProof.profile_id}
        options={profileOptions}
        disabled={disabled || proofProfiles.length === 0}
        onSelect={(value) => {
          if (!value) return;
          void applySoftProof({ ...softProof, profile_id: value });
        }}
      />
      <DropdownMenu
        label="Intent"
        value={softProof.intent}
        options={INTENT_OPTIONS}
        disabled={disabled}
        onSelect={(value) =>
          void applySoftProof({
            ...softProof,
            intent: value as SoftProofIntent,
          })
        }
      />
      <label
        className={cn('param-checkbox-row', 'sp-bpc-row')}
        title="Black-point compensation (Relative intent). App-owned until the CMS exposes a native flag."
      >
        <input
          type="checkbox"
          checked={softProof.bpc}
          disabled={disabled || softProof.intent === 'absolute'}
          onChange={(e) =>
            void applySoftProof({ ...softProof, bpc: e.target.checked })
          }
        />
        <span>BPC</span>
      </label>
      <p className={cn('sp-caveat')}>
        Soft proof assumes an sRGB display. Wide-gamut WebViews may not match print
        soft-proofing on calibrated hardware.
      </p>
      {missingProfileToast && <div className={cn('sp-warn')}>{missingProfileToast}</div>}
      {loadError && <div className={cn('sp-error')}>{loadError}</div>}
      <div className={cn('sp-actions')}>
        <button
          type="button"
          className={cn('color-lab-button', 'sp-action-btn')}
          disabled={disabled}
          onClick={() => void importProofProfile()}
        >
          Import…
        </button>
        {layout === 'footer' && (
          <button
            type="button"
            className={cn('color-lab-button', 'sp-action-btn')}
            onClick={() => setSettingsOpen(false)}
          >
            Close
          </button>
        )}
      </div>
    </div>
  );

  if (layout === 'panel') {
    return (
      <section className={cn('sp-section')} aria-label="Soft proof">
        <div className={cn('sp-section-head')}>
          <span className={cn('sp-section-title')}>Soft proof</span>
          <Tooltip label="Simulate print CMYK appearance on the Preview (display only)">
            <button
              type="button"
              className={cn(
                'color-lab-button',
                'sp-toggle-btn',
                softProof.enabled && 'sp-toggle-btn-active',
              )}
              aria-pressed={softProof.enabled}
              disabled={disabled}
              onClick={() => void applySoftProof({ ...softProof, enabled: !softProof.enabled })}
            >
              {softProof.enabled ? 'On' : 'Off'}
            </button>
          </Tooltip>
        </div>
        {softProof.enabled && (
          <div className={cn('sp-chip')} title={softProofChipLabel(softProof, proofProfiles)}>
            {softProofChipLabel(softProof, proofProfiles)}
          </div>
        )}
        {softProof.enabled && showStrictWarn && (
          <div className={cn('sp-warn')}>Screen ≠ palette</div>
        )}
        {settings}
      </section>
    );
  }

  return (
    <>
      <span className={cn('sp-footer')} role="group" aria-label="Soft proof">
        <Tooltip label="Simulate print CMYK appearance (display only)">
          <button
            type="button"
            className={cn('sp-footer-btn', softProof.enabled && 'sp-footer-btn-active')}
            aria-pressed={softProof.enabled}
            disabled={disabled}
            onClick={() => void applySoftProof({ ...softProof, enabled: !softProof.enabled })}
          >
            Soft proof
          </button>
        </Tooltip>
        <Tooltip label="Soft proof settings">
          <button
            type="button"
            className={cn('sp-footer-gear')}
            aria-expanded={settingsOpen}
            aria-label="Soft proof settings"
            disabled={disabled}
            onClick={() => setSettingsOpen((v) => !v)}
          >
            ▾
          </button>
        </Tooltip>
      </span>
      {settingsOpen && settings}
    </>
  );
}
