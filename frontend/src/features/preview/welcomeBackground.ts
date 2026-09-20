import type { CSSProperties } from 'react';

export type WelcomeBackground = 'artwork' | 'gradient';

export const DEFAULT_WELCOME_BACKGROUND: WelcomeBackground = 'artwork';

export const WELCOME_BACKGROUNDS: { id: WelcomeBackground; label: string }[] = [
  { id: 'artwork', label: 'Artwork' },
  { id: 'gradient', label: 'Gradient' },
];

export function parseWelcomeBackground(value: unknown): WelcomeBackground {
  if (value === 'artwork' || value === 'gradient') return value;
  // Former ids collapse to the default option.
  return DEFAULT_WELCOME_BACKGROUND;
}

function coverImageStyle(src: string): CSSProperties {
  return {
    backgroundColor: '#000000',
    backgroundImage: `url(${src})`,
    backgroundSize: 'cover',
    backgroundRepeat: 'no-repeat',
    backgroundPosition: 'center',
    imageRendering: 'pixelated',
  };
}

/** Visual for the welcome / empty-state surface. */
export function welcomeBackgroundStyle(kind: WelcomeBackground = DEFAULT_WELCOME_BACKGROUND): CSSProperties {
  if (kind === 'gradient') {
    return coverImageStyle('/img/background-img-2.png');
  }
  return coverImageStyle('/img/background-img-1.png');
}
