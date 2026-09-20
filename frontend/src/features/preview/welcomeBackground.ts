import type { CSSProperties } from 'react';

/** Only welcome background shipped for now. */
export type WelcomeBackground = 'artwork';

export const DEFAULT_WELCOME_BACKGROUND: WelcomeBackground = 'artwork';

export const WELCOME_BACKGROUNDS: { id: WelcomeBackground; label: string }[] = [
  { id: 'artwork', label: 'Artwork' },
];

export function parseWelcomeBackground(value: unknown): WelcomeBackground {
  if (value === 'artwork') return value;
  // Former ids collapse to the single remaining option.
  return DEFAULT_WELCOME_BACKGROUND;
}

/** Visual for the welcome / empty-state surface. */
export function welcomeBackgroundStyle(_kind?: WelcomeBackground): CSSProperties {
  return {
    backgroundColor: '#000000',
    backgroundImage: 'url(/img/background-img-1.png)',
    backgroundSize: 'cover',
    backgroundRepeat: 'no-repeat',
    backgroundPosition: 'center',
    imageRendering: 'pixelated',
  };
}
