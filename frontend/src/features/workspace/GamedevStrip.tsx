/**
 * Gamedev settings body — Color Lab section / Soft-proof toggle language.
 * Release slice: Tile + Sprites + Output (Animation deferred).
 *
 * Workflow order (top → bottom): pick a job → dither in Effect → read status →
 * inspect in preview footer → export. Copy stays short; detail in tooltips.
 */

import { useCallback } from 'react';
import DropdownMenu from '../../components/common/DropdownMenu';
import SpinNumberInput from '../../components/common/SpinNumberInput';
import type { GamedevChip, GamedevChipId } from '../../shared/ipc/gamedev';
import { bind } from '../../shared/ui/cn';
import Tooltip from '../../shared/ui/Tooltip';
import paramStyles from '../../shared/ui/ParamControls.module.css';
import ColorLabSection from '../color-lab/ColorLabSection';
import colorLabStyles from '../color-lab/ColorLabWindow.module.css';
import colorLabButtons from '../color-lab/ColorLabButtons.module.css';
import styles from './GamedevStrip.module.css';
import { useGamedevStore } from './GamedevContext';

const cn = bind({
  ...colorLabStyles,
  ...colorLabButtons,
  ...paramStyles,
  ...styles,
});

const CELL_MAX = 4096;

type JobKind = 'none' | 'tile' | 'sprites';

const UPSCALE_OPTIONS = [
  { value: '1', label: '1×' },
  { value: '2', label: '2×' },
  { value: '3', label: '3×' },
  { value: '4', label: '4×' },
];

const CHIP_TITLE: Record<GamedevChipId, string> = {
  seamless: 'Seamless',
  cells: 'Cells',
  stable: 'Stable',
  palette: 'Palette',
  alpha: 'Alpha',
};

/** Short status line under the chip name — no engine jargon. */
function chipStatusText(chip: GamedevChip): string {
  if (chip.state === 'pending') return 'checking…';

  const raw = chip.label.trim();
  const lower = raw.toLowerCase();

  if (lower === 'seamless · off') return 'off';
  if (lower === 'cells · off') return 'off';
  if (lower.includes('independent') && raw.includes('✓')) return 'independent';
  if (lower === 'cells · shared') return 'shared pass';
  if (lower.startsWith('cells · crop')) {
    return raw.slice(raw.indexOf('crop')).trim();
  }
  if (lower === 'tile · too large' || lower.includes('too large')) return 'too large';
  if (lower.startsWith('seam · snap')) {
    return raw.slice(raw.indexOf('snap')).trim();
  }
  if (lower === 'seam · mismatch') return 'mismatch';
  if (lower.startsWith('seamless ≈')) return raw.replace(/^seamless\s+/i, '').trim();
  if (raw.includes('✓') && lower.startsWith('seamless')) return 'verified';
  if (lower.startsWith('alpha · soft')) return 'soft edges';
  if (lower === 'alpha ✓') return 'clean';
  if (lower === 'alpha · dither') return 'dithering alpha';

  const sep = raw.indexOf('·');
  if (sep >= 0) {
    const detail = raw.slice(sep + 1).trim();
    if (detail) return detail;
  }
  const spaced = raw.match(/\s+(\S+)$/);
  if (spaced) return spaced[1];
  return raw;
}

function StatusRow({
  chip,
  onAction,
}: {
  chip: GamedevChip;
  onAction: (id: string) => void;
}) {
  const hasAction = Boolean(chip.actionId && chip.actionLabel);
  return (
    <div
      className={cn('statusRow')}
      data-chip-state={chip.state}
      title={chip.tooltip}
      data-testid={`gamedev-chip-${chip.id}`}
    >
      <span className={cn('statusDot')} aria-hidden />
      <div className={cn('statusText')}>
        <span className={cn('statusName')}>{CHIP_TITLE[chip.id] ?? chip.id}</span>
        <span className={cn('statusDetail')}>{chipStatusText(chip)}</span>
      </div>
      {hasAction ? (
        <button
          type="button"
          className={cn('color-lab-button', 'statusAction')}
          onClick={() => onAction(chip.actionId!)}
        >
          {chip.actionLabel}
        </button>
      ) : null}
    </div>
  );
}

function chipById(chips: GamedevChip[], id: GamedevChipId): GamedevChip | undefined {
  return chips.find((c) => c.id === id);
}

/** Chips that matter for the active job — hide “off” noise from the other job. */
function chipsForJob(chips: GamedevChip[], job: JobKind): GamedevChip[] {
  const order: GamedevChipId[] =
    job === 'tile' ? ['seamless', 'alpha'] : job === 'sprites' ? ['cells', 'alpha'] : [];
  return order.map((id) => chipById(chips, id)).filter((c): c is GamedevChip => c != null);
}

export function GamedevStrip() {
  const {
    context,
    chips,
    setPipelineTile,
    setCellSettings,
    setNearestPreview,
    setIntegerUpscale,
    setExportPng8,
    runChipAction,
  } = useGamedevStore();

  const cell = context.cell;
  const spritesOn = cell != null;
  const tileOn = Boolean(context.tile.enabled);
  const job: JobKind = tileOn ? 'tile' : spritesOn ? 'sprites' : 'none';

  const patchCell = useCallback(
    (patch: Partial<NonNullable<typeof cell>>) => {
      void setCellSettings({
        w: patch.w ?? cell?.w ?? 32,
        h: patch.h ?? cell?.h ?? 32,
        padding: patch.padding ?? cell?.padding ?? 0,
        independent: patch.independent ?? cell?.independent ?? true,
      });
    },
    [cell, setCellSettings],
  );

  const selectJob = useCallback(
    (next: JobKind) => {
      if (next === 'tile') {
        void setPipelineTile(true);
        return;
      }
      if (next === 'sprites') {
        void setCellSettings({
          w: cell?.w ?? 32,
          h: cell?.h ?? 32,
          padding: cell?.padding ?? 0,
          independent: cell?.independent ?? true,
        });
        return;
      }
      if (tileOn) void setPipelineTile(false);
      else if (spritesOn) void setCellSettings(null);
    },
    [cell, setCellSettings, setPipelineTile, spritesOn, tileOn],
  );

  const upscaleValue = String(context.pixel_perfect?.integer_upscale ?? 1);
  const visibleChips = chipsForJob(chips, job);

  return (
    <div
      className={cn('color-lab-body', 'color-lab-body-sidebar', 'gamedevBody')}
      data-testid="gamedev-strip"
      aria-label="Gamedev Settings"
    >
      <ol className={cn('workflow')} aria-label="How to use Gamedev">
        <li>
          <span className={cn('workflowStep')}>1</span>
          Pick a job below
        </li>
        <li>
          <span className={cn('workflowStep')}>2</span>
          Dither in the Effect panel
        </li>
        <li>
          <span className={cn('workflowStep')}>3</span>
          Read status, then inspect in the preview footer
        </li>
        <li>
          <span className={cn('workflowStep')}>4</span>
          Export (File → Export Image or Batch)
        </li>
      </ol>

      <div data-testid="gamedev-group-job">
        <ColorLabSection title="job" defaultOpen>
          <div className={cn('jobSegment')} role="group" aria-label="Gamedev job">
            <Tooltip label="Make opposite edges match so the texture tiles. Not the same as Check seams in the preview footer (display only).">
              <button
                type="button"
                className={cn('color-lab-button', 'jobSegBtn', job === 'tile' && 'jobSegBtnOn')}
                aria-pressed={job === 'tile'}
                data-testid="gamedev-tile-toggle"
                onClick={() => selectJob(job === 'tile' ? 'none' : 'tile')}
              >
                Tile
              </button>
            </Tooltip>
            <Tooltip label="Set sprite cell size for this document. Export and Batch read it here.">
              <button
                type="button"
                className={cn('color-lab-button', 'jobSegBtn', job === 'sprites' && 'jobSegBtnOn')}
                aria-pressed={job === 'sprites'}
                data-testid="gamedev-sprites-toggle"
                onClick={() => selectJob(job === 'sprites' ? 'none' : 'sprites')}
              >
                Sprites
              </button>
            </Tooltip>
            <Tooltip label="Turn off Tile and Sprites.">
              <button
                type="button"
                className={cn('color-lab-button', 'jobSegBtn', job === 'none' && 'jobSegBtnOn')}
                aria-pressed={job === 'none'}
                data-testid="gamedev-job-none"
                onClick={() => selectJob('none')}
              >
                Off
              </button>
            </Tooltip>
          </div>

          {job === 'none' ? (
            <p className={cn('jobLine')}>
              Tile = seamless textures. Sprites = sprite-sheet cells. One job at a time.
            </p>
          ) : null}

          {job === 'tile' ? (
            <p className={cn('jobLine')}>
              Edges are dithered to match. After Effect updates, check Seamless below, then use{' '}
              <strong>Check seams</strong> in the preview footer.
            </p>
          ) : null}

          {job === 'sprites' ? (
            <>
              <div className={cn('cellRow')}>
                <label className={cn('fieldLabel')}>size</label>
                <SpinNumberInput
                  label="Cell width"
                  value={cell?.w ?? 32}
                  min={1}
                  max={CELL_MAX}
                  testId="gamedev-cell-w"
                  onChange={(w) => patchCell({ w })}
                />
                <span className={cn('fieldSep')} aria-hidden>
                  ×
                </span>
                <SpinNumberInput
                  label="Cell height"
                  value={cell?.h ?? 32}
                  min={1}
                  max={CELL_MAX}
                  testId="gamedev-cell-h"
                  onChange={(h) => patchCell({ h })}
                />
              </div>
              <div className={cn('cellRow')}>
                <label
                  className={cn('fieldLabel')}
                  title="Extra pixels from neighbors used while dithering; not written into the cell."
                >
                  pad
                </label>
                <SpinNumberInput
                  label="Cell padding"
                  value={cell?.padding ?? 0}
                  min={0}
                  max={CELL_MAX}
                  testId="gamedev-cell-pad"
                  onChange={(padding) => patchCell({ padding })}
                />
              </div>
              <label
                className={cn('param-checkbox-row', 'jobCheckbox')}
                title="Dither each cell alone so neighboring cells cannot bleed into each other."
              >
                <input
                  type="checkbox"
                  checked={cell?.independent ?? true}
                  onChange={(e) => patchCell({ independent: e.target.checked })}
                />
                Keep cells separate
              </label>
              <p className={cn('jobLine')}>
                After Effect updates, check Cells below, then use <strong>Cell grid</strong> in the
                preview footer.
              </p>
            </>
          ) : null}
        </ColorLabSection>
      </div>

      <div data-testid="gamedev-group-status">
        <ColorLabSection title="status" defaultOpen>
          {job === 'none' ? (
            <p className={cn('jobLine')}>Pick Tile or Sprites to see checks for that job.</p>
          ) : visibleChips.length === 0 ? (
            <p className={cn('jobLine')}>Checks appear here after you dither.</p>
          ) : (
            <div className={cn('statusList')}>
              {visibleChips.map((chip) => (
                <StatusRow
                  key={chip.id}
                  chip={chip}
                  onAction={(actionId) => void runChipAction(actionId)}
                />
              ))}
            </div>
          )}
        </ColorLabSection>
      </div>

      <div data-testid="gamedev-options">
        <ColorLabSection title="output" defaultOpen>
          <label
            className={cn('param-checkbox-row', 'jobCheckbox')}
            title="Scale the preview with sharp pixels (nearest neighbor). Does not change export."
          >
            <input
              type="checkbox"
              checked={Boolean(context.pixel_perfect?.nearest_preview)}
              onChange={(e) => void setNearestPreview(e.target.checked)}
              data-testid="gamedev-nearest-preview"
            />
            Sharp preview
          </label>
          <label
            className={cn('param-checkbox-row', 'jobCheckbox')}
            title="Prefer indexed PNG8 when you Export or Batch from this document."
          >
            <input
              type="checkbox"
              checked={Boolean(context.export?.png8)}
              onChange={(e) => void setExportPng8(e.target.checked)}
              data-testid="gamedev-export-png8"
            />
            Indexed PNG
          </label>
          <div data-testid="gamedev-integer-upscale">
            <Tooltip label="Dither at canvas÷N, then scale up with sharp pixels. Width and height must divide evenly by N.">
              <DropdownMenu
                label="export scale"
                value={upscaleValue}
                options={UPSCALE_OPTIONS}
                ariaLabel="Export scale"
                onSelect={(v) => {
                  const n = Number(v);
                  void setIntegerUpscale(n <= 1 ? null : n);
                }}
              />
            </Tooltip>
          </div>
        </ColorLabSection>
      </div>
    </div>
  );
}
