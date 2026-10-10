import { useCallback, useEffect, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import styles from '../features/document/NewProjectDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import {
  proofGetConfig,
  proofListProfiles,
  type ProofProfileInfo,
  type SoftProofIntent,
} from '../shared/ipc/proof';
import {
  printExportEstimate,
  printExportGamutReport,
  type GamutReport,
  type PrintExportConfig,
  type PrintExportEstimate,
  type TiffCompression,
} from '../shared/ipc/printExport';

const cn = bind(styles);

const INTENT_OPTIONS: { value: SoftProofIntent; label: string }[] = [
  { value: 'relative', label: 'Relative' },
  { value: 'perceptual', label: 'Perceptual' },
  { value: 'absolute', label: 'Absolute' },
];

export interface ExportPrintDialogProps {
  isOpen: boolean;
  docId: number | null;
  docWidth: number;
  docHeight: number;
  onExport: (config: PrintExportConfig) => void;
  onClose: () => void;
}

export default function ExportPrintDialog({
  isOpen,
  docId,
  docWidth,
  docHeight,
  onExport,
  onClose,
}: ExportPrintDialogProps) {
  const [profiles, setProfiles] = useState<ProofProfileInfo[]>([]);
  const [profileId, setProfileId] = useState('builtin:fogra51');
  const [intent, setIntent] = useState<SoftProofIntent>('relative');
  const [bpc, setBpc] = useState(true);
  const [ppi, setPpi] = useState(300);
  const [scale, setScale] = useState(1);
  const [pureBlackK, setPureBlackK] = useState(true);
  const [compression, setCompression] = useState<TiffCompression>('lzw');
  const [estimate, setEstimate] = useState<PrintExportEstimate | null>(null);
  const [gamut, setGamut] = useState<GamutReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const config: PrintExportConfig = useMemo(
    () => ({
      format: 'tiff',
      profile_id: profileId,
      intent,
      bpc,
      ppi,
      scale,
      pure_black_k: pureBlackK,
      compression,
    }),
    [profileId, intent, bpc, ppi, scale, pureBlackK, compression]
  );

  useEffect(() => {
    if (!isOpen || docId == null) return;
    let cancelled = false;
    void (async () => {
      try {
        const [list, soft] = await Promise.all([proofListProfiles(), proofGetConfig(docId)]);
        if (cancelled) return;
        setProfiles(list);
        setProfileId(soft.profile_id || 'builtin:fogra51');
        setIntent(soft.intent);
        setBpc(soft.bpc);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [isOpen, docId]);

  useEffect(() => {
    if (!isOpen || docId == null) return;
    let cancelled = false;
    setBusy(true);
    setError(null);
    const t = window.setTimeout(() => {
      void (async () => {
        try {
          const est = await printExportEstimate(docId, config);
          if (cancelled) return;
          setEstimate(est);
          const report = await printExportGamutReport(docId, config);
          if (cancelled) return;
          setGamut(report);
        } catch (e) {
          if (!cancelled) setError(String(e));
        } finally {
          if (!cancelled) setBusy(false);
        }
      })();
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [isOpen, docId, config]);

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  if (!isOpen) return null;

  const outW = estimate?.out_width ?? docWidth * scale;
  const outH = estimate?.out_height ?? docHeight * scale;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="print-export-overlay"
    >
      <div
        className={cn('new-project-dialog', 'new-project-dialog-wide')}
        role="dialog"
        aria-modal="true"
        aria-label="Export for Print"
      >
        <DialogTitlebar title="Export for Print" onClose={onClose} />
        <form
          className={cn('new-project-body')}
          style={{ padding: '14px 12px 12px', overflow: 'auto' }}
          onSubmit={(e) => {
            e.preventDefault();
            onExport(config);
          }}
        >
          <p className={cn('new-project-hint')} style={{ margin: 0, opacity: 0.85 }}>
            Screen ≠ print. Soft proof does not change this file — check a press proof at the shop.
          </p>

          <fieldset className={cn('new-project-field')}>
            <legend>Format</legend>
            <label className={cn('new-project-radio')}>
              <input type="radio" checked readOnly />
              CMYK TIFF (.tif) — 8-bit, embedded ICC
            </label>
          </fieldset>

          <label className={cn('new-project-field')}>
            Profile
            <select
              value={profileId}
              onChange={(e) => setProfileId(e.target.value)}
              className={cn('new-project-input')}
            >
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </label>

          <label className={cn('new-project-field')}>
            Intent
            <select
              value={intent}
              onChange={(e) => setIntent(e.target.value as SoftProofIntent)}
              className={cn('new-project-input')}
            >
              {INTENT_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>

          <label className={cn('new-project-checkbox')}>
            <input type="checkbox" checked={bpc} onChange={(e) => setBpc(e.target.checked)} />
            Black-point compensation (Relative)
          </label>

          <label className={cn('new-project-checkbox')}>
            <input
              type="checkbox"
              checked={pureBlackK}
              onChange={(e) => setPureBlackK(e.target.checked)}
            />
            Pure black on K only (RGB 0,0,0 → 0,0,0,100)
          </label>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
            <label className={cn('new-project-field')}>
              PPI
              <input
                type="number"
                min={1}
                step={1}
                value={ppi}
                onChange={(e) => setPpi(Math.max(1, Number(e.target.value) || 1))}
                className={cn('new-project-input')}
              />
            </label>
            <label className={cn('new-project-field')}>
              Integer scale
              <input
                type="number"
                min={1}
                max={16}
                step={1}
                value={scale}
                onChange={(e) => setScale(Math.max(1, Math.min(16, Number(e.target.value) || 1)))}
                className={cn('new-project-input')}
              />
            </label>
          </div>

          <fieldset className={cn('new-project-field')}>
            <legend>TIFF compression</legend>
            <label className={cn('new-project-radio')}>
              <input
                type="radio"
                name="tiff-compression"
                checked={compression === 'lzw'}
                onChange={() => setCompression('lzw')}
              />
              LZW (lossless)
            </label>
            <label className={cn('new-project-radio')}>
              <input
                type="radio"
                name="tiff-compression"
                checked={compression === 'none'}
                onChange={() => setCompression('none')}
              />
              None
            </label>
          </fieldset>

          <div className={cn('new-project-field')} style={{ fontSize: 12, opacity: 0.9 }}>
            <div>
              Output: {outW} × {outH} px
              {estimate
                ? ` · ${estimate.width_mm.toFixed(1)} × ${estimate.height_mm.toFixed(1)} mm @ ${ppi} PPI`
                : null}
            </div>
            {estimate ? (
              <div>
                Unique colors: {estimate.unique_colors}
                {estimate.uses_palette_path ? ' (exact palette path)' : ' (pointwise CMS)'}
              </div>
            ) : null}
            {busy ? <div>Updating gamut report…</div> : null}
            {gamut ? (
              <div>
                Out-of-gamut pixels: {(gamut.out_of_gamut_fraction * 100).toFixed(1)}%
                {gamut.warning_palette_collapse
                  ? ' · Warning: palette colors collapse to the same CMYK'
                  : null}
                {' · '}Max ink: {gamut.max_ink_coverage.toFixed(0)}%
              </div>
            ) : null}
            {error ? <div style={{ color: 'var(--danger, #b33)' }}>{error}</div> : null}
          </div>

          <div className={cn('new-project-footer')}>
            <button type="button" className={cn('new-project-btn')} onClick={onClose}>
              Cancel
            </button>
            <button
              type="submit"
              className={cn('new-project-btn', 'new-project-btn-primary')}
              disabled={!!error && !estimate}
            >
              Export…
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body
  );
}
