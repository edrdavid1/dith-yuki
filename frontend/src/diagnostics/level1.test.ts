import { afterEach, describe, expect, it } from 'vitest';
import {
  deriveLevel1,
  formatLevel1,
  gestureSnapshot,
  noteCommitTimeout,
  noteDisplayPost,
  noteFramePainted,
  noteIpcDone,
  noteIpcSent,
  noteTileDecoded,
  noteUserInput,
  resetLevel1ForTests,
  stampWorkerMessage,
  type GestureMarks,
} from './level1';
import { diagRingLines, pushDiagLine, RING_CAP, setPreviewDiagnostics } from './session';

function phasesOf(partial: Partial<GestureMarks> & Pick<GestureMarks, 'T_input'>): GestureMarks {
  return {
    session: 's',
    scenario: 'slider',
    doc: 1,
    viewport: 'z1.00_1920x1080',
    algo: 'bayer_8x8',
    T_ipc_sent: 150,
    T_ipc_done: 154,
    T_first_post: 160,
    T_first_decoded: 300,
    T_first_draw: 308,
    T_last_decoded: 900,
    T_last_draw: 912,
    T_css_pan: null,
    update_filter_calls: 1,
    set_viewport_calls: 0,
    display_tiles: 2,
    decoded_tiles: 2,
    http_202: 0,
    stale_hits: 0,
    pyramid_fallbacks: 0,
    closed_by: 'all_visible',
    expected: ['0/0/0', '0/1/0'],
    decodedKeys: ['0/0/0', '0/1/0'],
    ...partial,
  };
}

describe('preview diagnostics level 1', () => {
  afterEach(() => {
    resetLevel1ForTests();
  });

  it('derives phases that sum to total_ms', () => {
    const marks = phasesOf({ T_input: 0, T_css_pan: 4 });
    const phases = deriveLevel1(marks);
    expect(phases).not.toBeNull();
    const sum =
      phases!.debounce_ms +
      phases!.param_ipc_ms +
      phases!.backend_wait_ms +
      phases!.spread_ms +
      phases!.commit_draw_ms;
    expect(Math.abs(sum - phases!.total_ms)).toBeLessThanOrEqual(2);
    expect(phases!.css_pan_ms).toBe(4);
    expect(phases!.css_pan_ms).not.toBe(phases!.total_ms);
  });

  it('keeps marks ordered on one clock', () => {
    setPreviewDiagnostics(true, 1);
    noteUserInput('slider', 'bayer_8x8');
    noteIpcSent('update_filter');
    noteIpcDone();
    noteDisplayPost(['0/0/0', '0/1/0'], 1, 1, 1920, 1080);
    noteTileDecoded('0/0/0');
    noteTileDecoded('0/1/0');
    noteFramePainted();
    const g = gestureSnapshot();
    expect(g).not.toBeNull();
    const stamps = [
      g!.T_input,
      g!.T_ipc_sent!,
      g!.T_ipc_done!,
      g!.T_first_post!,
      g!.T_first_decoded!,
      g!.T_last_decoded!,
      g!.T_last_draw!,
    ];
    for (let i = 1; i < stamps.length; i++) {
      expect(stamps[i]).toBeGreaterThanOrEqual(stamps[i - 1]);
    }
    const phases = deriveLevel1(g!);
    expect(phases).not.toBeNull();
    const line = formatLevel1(g!, phases!);
    expect(line.startsWith('[diag/L1] ')).toBe(true);
    expect(diagRingLines().some((l) => l.includes(g!.session))).toBe(true);
  });

  it('issues a new session id per gesture', () => {
    setPreviewDiagnostics(true, 1);
    noteUserInput('slider');
    const first = gestureSnapshot()!.session;
    noteUserInput('zoom');
    const second = gestureSnapshot()!.session;
    expect(first).not.toBe(second);
  });

  it('counts each tile once inside a gesture', () => {
    setPreviewDiagnostics(true, 1);
    noteUserInput('slider');
    noteDisplayPost(['0/0/0'], 1, 1, 1920, 1080);
    noteTileDecoded('0/0/0', { retries: 2 });
    noteTileDecoded('0/0/0', { retries: 2 });
    noteTileDecoded('0/9/9');
    const g = gestureSnapshot()!;
    expect(g.decoded_tiles).toBe(1);
    expect(g.http_202).toBe(2);
    expect(g.decodedKeys).toEqual(['0/0/0']);
  });

  it('closes on commit timeout', () => {
    setPreviewDiagnostics(true, 1);
    noteUserInput('slider');
    noteIpcSent('update_filter');
    noteIpcDone();
    noteDisplayPost(['0/0/0', '0/1/0'], 1, 1, 1920, 1080);
    noteTileDecoded('0/0/0');
    noteCommitTimeout();
    noteFramePainted();
    const g = gestureSnapshot()!;
    expect(g.closed_by).toBe('commit_timeout');
    expect(deriveLevel1(g)).not.toBeNull();
    expect(diagRingLines().length).toBe(1);
  });

  it('does not stamp worker messages while disabled', () => {
    const msg = { type: 'fetch-tile', level: 0, x: 1, y: 2, docId: 4 };
    const stamped = stampWorkerMessage(msg);
    expect(stamped).toBe(msg);
    expect(Object.keys(stamped)).toEqual(['type', 'level', 'x', 'y', 'docId']);
    setPreviewDiagnostics(true, 1);
    const on = stampWorkerMessage(msg);
    expect(on.diag).toBe(true);
  });

  it('drops the oldest ring line past the cap', () => {
    for (let i = 0; i < RING_CAP + 1; i++) pushDiagLine(`line-${i}`);
    const lines = diagRingLines();
    expect(lines).toHaveLength(RING_CAP);
    expect(lines[0]).toBe('line-1');
    expect(lines[lines.length - 1]).toBe(`line-${RING_CAP}`);
  });
});
