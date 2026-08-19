import type { CSSProperties } from 'react';

export type PreviewBackground = 'gray' | 'black' | 'pattern';

export const DEFAULT_PREVIEW_BACKGROUND: PreviewBackground = 'gray';

export const PREVIEW_BACKGROUNDS: { id: PreviewBackground; label: string }[] = [
  { id: 'gray', label: 'Gray' },
  { id: 'black', label: 'Black' },
  { id: 'pattern', label: 'Pattern' },
];

export function parsePreviewBackground(value: unknown): PreviewBackground {
  if (value === 'gray' || value === 'black' || value === 'pattern') return value;
  if (typeof value === 'string') {
    const hex = value.trim().toLowerCase();
    if (hex === '#000' || hex === '#000000') return 'black';
  }
  return DEFAULT_PREVIEW_BACKGROUND;
}

/** Visual for CSS surfaces behind the canvas. */
export function previewBackgroundStyle(kind: PreviewBackground): CSSProperties {
  if (kind === 'black') {
    return { background: '#000000' };
  }
  if (kind === 'pattern') {
    return {
      backgroundColor: '#808080',
      backgroundImage: 'url(/img/preview-halftone.png)',
      backgroundRepeat: 'repeat',
      imageRendering: 'pixelated',
    };
  }
  return { background: '#999999' };
}

let halftoneImage: HTMLImageElement | null = null;
let halftoneLoad: Promise<HTMLImageElement> | null = null;

export function loadHalftoneImage(): Promise<HTMLImageElement> {
  if (halftoneImage) return Promise.resolve(halftoneImage);
  if (!halftoneLoad) {
    halftoneLoad = new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        halftoneImage = img;
        resolve(img);
      };
      img.onerror = () => reject(new Error('Failed to load preview halftone'));
      img.src = '/img/preview-halftone.png';
    });
  }
  return halftoneLoad;
}

/** Fill the tile canvas backing store so the chosen preview bg is visible around the image. */
export function fillPreviewCanvasBackground(
  ctx: CanvasRenderingContext2D,
  kind: PreviewBackground,
  dpr: number,
  width: number,
  height: number
): void {
  if (kind === 'pattern' && halftoneImage) {
    const pat = ctx.createPattern(halftoneImage, 'repeat');
    if (pat) {
      pat.setTransform(new DOMMatrix().scale(dpr, dpr));
      ctx.fillStyle = pat;
      ctx.fillRect(0, 0, width, height);
      return;
    }
  }
  ctx.fillStyle = kind === 'black' ? '#000000' : '#999999';
  ctx.fillRect(0, 0, width, height);
}
