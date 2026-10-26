/**
 * Corner "Rendering…" vs subtle recomputing chip.
 *
 * Overlay only while busy AND no frame has been shown yet for this document
 * (including a retained prior frame after content_changed).
 */
export function shouldShowRenderingOverlay(
  fullDocumentBusy: boolean,
  hadPreviewFrame: boolean,
): boolean {
  return fullDocumentBusy && !hadPreviewFrame;
}

export function shouldShowRecomputingChip(
  fullDocumentBusy: boolean,
  hadPreviewFrame: boolean,
): boolean {
  return fullDocumentBusy && hadPreviewFrame;
}

/**
 * Sequence helper for tests: open → add (no busy) → update #2 (busy) → #3 (busy).
 * After any tile has been drawn, #2/#3 must not show the overlay.
 */
export function overlaySequenceAfterFirstFrame(args: {
  /** True once any tile has been drawn for this doc (kept frame counts). */
  hadPreviewFrame: boolean;
  /** Busy during update waves. */
  updateBusy: boolean;
}): { overlay: boolean; chip: boolean } {
  return {
    overlay: shouldShowRenderingOverlay(args.updateBusy, args.hadPreviewFrame),
    chip: shouldShowRecomputingChip(args.updateBusy, args.hadPreviewFrame),
  };
}
