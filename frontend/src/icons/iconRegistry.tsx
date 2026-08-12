import React from 'react';

export type IconName =
  | 'effect.dithering'
  | 'effect.glitching'
  | 'effect.curves'
  | 'effect.rgb'
  | 'image.source'
  | 'plus'
  | 'trash'
  | 'open-eye'
  ;

// Default mapping — points to files in `public/icons` so they work both in dev and
// in the built `dist` directory. This mapping can be swapped at runtime using
// `setIconMap`, following an i18-style indirection.
export const defaultIconMap: Record<string, string> = {
  'effect.dithering': '/icons/dethering-icon.svg',
  'effect.glitching': '/icons/glitching-icon.svg',
  'effect.curves': '/icons/curves-icon.svg',
  'effect.rgb': '/icons/rgb-channel-icon.svg',
  'image.source': '/icons/dethering-icon.svg',
  'plus': '/icons/plus-icon.svg',
  'trash': '/icons/trash-icon.svg',
  'open-eye': '/icons/open-eye-icon.svg',
  'dropdown-arrow': '/icons/dropdown-arrow-icon.svg',
  'row-img': '/icons/row-img-icon.svg',
  'import': '/icons/import-icon.svg',
  'export': '/icons/expot-icon.svg',
  'delete-con': '/icons/delete-con.svg',
  'sort': '/icons/sort-icon.svg',
  'auto-interpolate': '/icons/auto-interpolate-icon.svg',
  'slider-carret': '/icons/slider-carret-icon.svg',
};

let iconMap: Record<string, string> = { ...defaultIconMap };

export function setIconMap(map: Record<string, string>) {
  iconMap = { ...iconMap, ...map };
}

export function getIconSrc(name: string) {
  return iconMap[name] ?? name;
}

export function Icon({
  name,
  width = 24,
  height = 24,
  alt = '',
  style,
  className,
}: {
  name: string;
  width?: number | string;
  height?: number | string;
  alt?: string;
  style?: React.CSSProperties;
  className?: string;
}) {
  const src = getIconSrc(name);
  // If the mapping points to an inline SVG string (rare), the caller can pass
  // a full `<svg>` but by default we treat values as URLs and render <img>.
  return (
    // eslint-disable-next-line jsx-a11y/alt-text
    <img src={src} width={width} height={height} alt={alt} style={style} className={className} />
  );
}

export default Icon;
