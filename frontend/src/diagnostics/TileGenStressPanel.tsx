/**
 * Dev-only stress panel. Rendered only when DITHER_TRACE is on.
 */

import { useCallback, useEffect, useState } from 'react';
import {
  probeTileGenTrace,
  tileGenTraceEnabled,
} from './tileGenTrace';
import {
  runStressCampaign,
  STRESS_MODES,
  summarizeReports,
  type StressComputePath,
  type StressMode,
  type StressScenario,
  type SeriesReport,
} from './tileGenStressRunner';
import {
  runZoomScenario,
  summarizeZoomReports,
  type ZoomScenario,
} from './viewportStressRunner';

export type TileGenStressPanelProps = {
  baseParams: Record<string, unknown> | null;
  updateParams: (patch: Record<string, unknown>) => void;
  layerId: number | null;
  getVisibleKeys: () => string[];
  getViewportLevel: () => number;
  onPanZoom?: (step: number) => void;
};

export default function TileGenStressPanel({
  baseParams,
  updateParams,
  layerId,
  getVisibleKeys,
  getViewportLevel,
  onPanZoom,
}: TileGenStressPanelProps) {
  const [visible, setVisible] = useState(false);
  const [mode, setMode] = useState<StressMode>('floyd_steinberg');
  const [computePath, setComputePathState] = useState<StressComputePath>('cpu');
  const [scenario, setScenario] = useState<StressScenario | ZoomScenario>('S1');
  const [intervalMs, setIntervalMs] = useState(50);
  const [changes, setChanges] = useState(10);
  const [series, setSeries] = useState(3);
  const [seed, setSeed] = useState(42);
  const [running, setRunning] = useState(false);
  const [log, setLog] = useState('');
  const [lastReports, setLastReports] = useState<SeriesReport[]>([]);

  useEffect(() => {
    let cancelled = false;
    let attempts = 0;
    const tryProbe = () => {
      void probeTileGenTrace().then((on) => {
        if (cancelled) return;
        setVisible(on);
        attempts += 1;
        // Retry briefly in case Effects mounted before IPC was ready.
        if (!on && attempts < 12) {
          window.setTimeout(tryProbe, 500);
        }
      });
    };
    tryProbe();
    return () => {
      cancelled = true;
    };
  }, []);

  const isZoom = (s: string): s is ZoomScenario => s.startsWith('Z');

  const run = useCallback(async () => {
    if (!tileGenTraceEnabled()) {
      setLog('Need DITHER_TRACE=1.');
      return;
    }
    setRunning(true);
    setLog('Running…');
    try {
      if (isZoom(scenario)) {
        const reports = await runZoomScenario({
          scenario,
          series,
          seed,
          computePath,
          applyFilter:
            layerId != null && baseParams
              ? () =>
                  updateParams({
                    ...baseParams,
                    mode,
                    threshold_scale: 1.05,
                  })
              : undefined,
          noopParams:
            layerId != null && baseParams
              ? () => updateParams({ ...baseParams, mode })
              : undefined,
          changeParams:
            layerId != null && baseParams
              ? () =>
                  updateParams({
                    ...baseParams,
                    mode,
                    threshold_scale: 1.2,
                  })
              : undefined,
        });
        setLastReports([]);
        setLog(`${summarizeZoomReports(reports)} (path=${computePath}, mode=${mode})`);
      } else {
        if (layerId == null || !baseParams) {
          setLog('Need selected dither layer and params for S* scenarios.');
          return;
        }
        const reports = await runStressCampaign({
          mode,
          scenario: scenario as StressScenario,
          intervalMs,
          changes,
          series,
          seed,
          computePath,
          baseParams,
          updateParams,
          getVisibleKeys,
          getViewportLevel,
          layerId,
          onPanZoom,
        });
        setLastReports(reports);
        setLog(`${summarizeReports(reports)} (path=${computePath}, mode=${mode})`);
      }
    } catch (e) {
      setLog(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }, [
    baseParams,
    changes,
    computePath,
    getViewportLevel,
    getVisibleKeys,
    intervalMs,
    layerId,
    mode,
    onPanZoom,
    scenario,
    seed,
    series,
    updateParams,
  ]);

  if (!visible) return null;

  return (
    <div
      style={{
        margin: '8px 0',
        padding: '8px',
        border: '1px solid #888',
        fontSize: 12,
        fontFamily: 'ui-monospace, monospace',
        maxHeight: 320,
        overflow: 'auto',
      }}
    >
      <div style={{ fontWeight: 600, marginBottom: 6 }}>Стресс (DITHER_TRACE)</div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
        <label>
          path{' '}
          <select
            value={computePath}
            onChange={(e) => setComputePathState(e.target.value as StressComputePath)}
          >
            <option value="cpu">cpu</option>
            <option value="gpu">gpu</option>
            <option value="auto">auto</option>
          </select>
        </label>
        <label>
          mode{' '}
          <select value={mode} onChange={(e) => setMode(e.target.value as StressMode)}>
            <optgroup label="CPU ordered / Path B">
              {STRESS_MODES.filter((m) =>
                m.startsWith('bayer_') ||
                m.includes('ordered') ||
                m === 'halftone_screen_angled',
              ).map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </optgroup>
            <optgroup label="CPU error diffusion">
              {STRESS_MODES.filter((m) =>
                [
                  'floyd_steinberg',
                  'atkinson',
                  'jarvis_judice_ninke',
                  'stucki',
                  'burkes',
                  'fan93',
                  'ostromoukhov',
                  'sierra',
                  'sierra_lite',
                  'sierra_two_row',
                  'shiau_fan',
                  'stevenson_arce',
                  'zhou_fang',
                  'riemersma',
                ].includes(m),
              ).map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </optgroup>
            <optgroup label="CPU other">
              {STRESS_MODES.filter((m) =>
                [
                  'void_and_cluster',
                  'crosshatch_dither',
                  'line_screen',
                  'voronoi_stipple',
                  'random_dot_stipple',
                  'cmyk_halftone',
                  'wave',
                ].includes(m),
              ).map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </optgroup>
            <optgroup label="GPU-capable (use path=cpu/gpu)">
              <option value="blocked_fs">blocked_fs</option>
              <option value="blue_noise">blue_noise</option>
            </optgroup>
          </select>
        </label>
        <label>
          scenario{' '}
          <select
            value={scenario}
            onChange={(e) => {
              const next = e.target.value as StressScenario | ZoomScenario;
              setScenario(next);
              // Spec defaults: Z1/Z3 → CPU, Z2 → GPU (user can still override path).
              if (next === 'Z1' || next === 'Z3') setComputePathState('cpu');
              if (next === 'Z2') setComputePathState('gpu');
            }}
          >
            <option value="S1">S1</option>
            <option value="S2">S2</option>
            <option value="S3">S3</option>
            <option value="S5">S5</option>
            <option value="S8">S8</option>
            <option value="Z1">Z1 zoom-out (CPU)</option>
            <option value="Z2">Z2 zoom-out (GPU)</option>
            <option value="Z3">Z3 during recompute (CPU)</option>
            <option value="Z4">Z4 pan heal</option>
            <option value="Z5">Z5 no filter</option>
            <option value="Z6">Z6 mode bisect</option>
            <option value="Z7">Z7 what heals</option>
            <option value="Z_INJECT">Z inject VP_NOT_SENT</option>
          </select>
        </label>
        <label>
          interval
          <input
            type="number"
            value={intervalMs}
            min={0}
            style={{ width: 56 }}
            onChange={(e) => setIntervalMs(Number(e.target.value))}
          />
        </label>
        <label>
          changes
          <input
            type="number"
            value={changes}
            min={1}
            style={{ width: 48 }}
            onChange={(e) => setChanges(Number(e.target.value))}
          />
        </label>
        <label>
          series
          <input
            type="number"
            value={series}
            min={1}
            style={{ width: 40 }}
            onChange={(e) => setSeries(Number(e.target.value))}
          />
        </label>
        <label>
          seed
          <input
            type="number"
            value={seed}
            style={{ width: 64 }}
            onChange={(e) => setSeed(Number(e.target.value))}
          />
        </label>
        <button type="button" disabled={running} onClick={() => void run()}>
          {running ? '…' : 'Стресс'}
        </button>
      </div>
      <pre style={{ marginTop: 8, whiteSpace: 'pre-wrap' }}>{log}</pre>
      {lastReports.length > 0 && (
        <div style={{ opacity: 0.7 }}>
          last series violations:{' '}
          {lastReports[lastReports.length - 1]?.violations.length ?? 0}
        </div>
      )}
    </div>
  );
}
