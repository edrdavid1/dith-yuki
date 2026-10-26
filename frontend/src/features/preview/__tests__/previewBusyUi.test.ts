import { describe, expect, it } from 'vitest';
import {
  overlaySequenceAfterFirstFrame,
  shouldShowRecomputingChip,
  shouldShowRenderingOverlay,
} from '../previewBusyUi';

describe('previewBusyUi', () => {
  it('shows overlay only when busy and no frame yet', () => {
    expect(shouldShowRenderingOverlay(true, false)).toBe(true);
    expect(shouldShowRenderingOverlay(true, true)).toBe(false);
    expect(shouldShowRenderingOverlay(false, false)).toBe(false);
    expect(shouldShowRenderingOverlay(false, true)).toBe(false);
  });

  it('shows chip only when busy after a frame', () => {
    expect(shouldShowRecomputingChip(true, true)).toBe(true);
    expect(shouldShowRecomputingChip(true, false)).toBe(false);
    expect(shouldShowRecomputingChip(false, true)).toBe(false);
  });

  it('open → add → update #2 → #3: no overlay on #2 once a frame exists', () => {
    // After open/add, tiles are drawn (kept frame counts as hadPreviewFrame).
    const afterFirstPaint = overlaySequenceAfterFirstFrame({
      hadPreviewFrame: true,
      updateBusy: true,
    });
    expect(afterFirstPaint.overlay).toBe(false);
    expect(afterFirstPaint.chip).toBe(true);

    // #3 same: chip only.
    const update3 = overlaySequenceAfterFirstFrame({
      hadPreviewFrame: true,
      updateBusy: true,
    });
    expect(update3.overlay).toBe(false);
    expect(update3.chip).toBe(true);
  });

  it('edit before any frame still shows overlay', () => {
    const early = overlaySequenceAfterFirstFrame({
      hadPreviewFrame: false,
      updateBusy: true,
    });
    expect(early.overlay).toBe(true);
    expect(early.chip).toBe(false);
  });
});
