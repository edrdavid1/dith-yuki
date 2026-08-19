import React from 'react';
import { Chart } from 'pixelarticons/react/Chart';
import { Close } from 'pixelarticons/react/Close';
import { ColorsSwatch } from 'pixelarticons/react/ColorsSwatch';
import { Brush } from 'pixelarticons/react/Brush';
import { Delete } from 'pixelarticons/react/Delete';
import { Download } from 'pixelarticons/react/Download';
import { Expand } from 'pixelarticons/react/Expand';
import { Eye } from 'pixelarticons/react/Eye';
import { EyeOff } from 'pixelarticons/react/EyeOff';
import { Files } from 'pixelarticons/react/Files';
import { FlipHorizontal2 } from 'pixelarticons/react/FlipHorizontal2';
import { Frame } from 'pixelarticons/react/Frame';
import { GitMerge } from 'pixelarticons/react/GitMerge';
import { Grid3x3 } from 'pixelarticons/react/Grid3x3';
import { Image } from 'pixelarticons/react/Image';
import { ImageNew } from 'pixelarticons/react/ImageNew';
import { InfoBox } from 'pixelarticons/react/InfoBox';
import { Lightbulb } from 'pixelarticons/react/Lightbulb';
import { Minus } from 'pixelarticons/react/Minus';
import { Monitor } from 'pixelarticons/react/Monitor';
import { Plus } from 'pixelarticons/react/Plus';
import { Save } from 'pixelarticons/react/Save';
import { Settings2 } from 'pixelarticons/react/Settings2';
import { SettingsCog } from 'pixelarticons/react/SettingsCog';
import { SortVertical } from 'pixelarticons/react/SortVertical';
import { Trash } from 'pixelarticons/react/Trash';
import { Upload } from 'pixelarticons/react/Upload';
import { Zap } from 'pixelarticons/react/Zap';
import { ZoomIn } from 'pixelarticons/react/ZoomIn';
import { ZoomOut } from 'pixelarticons/react/ZoomOut';

export type IconName =
  | 'effect.dithering'
  | 'effect.glitching'
  | 'effect.curves'
  | 'effect.rgb'
  | 'effect.glow'
  | 'effect.crt'
  | 'effect.adjust'
  | 'image.source'
  | 'plus'
  | 'trash'
  | 'open-eye'
  | 'eye-off'
  | 'close'
  | 'row-img'
  | 'image-actual'
  | 'import'
  | 'export'
  | 'delete-con'
  | 'sort'
  | 'auto-interpolate'
  | 'zoom-in'
  | 'zoom-out'
  | 'zoom-fit'
  | 'zoom-1x'
  | 'zoom-integer'
  | 'color-lab'
  | 'preferences'
  | 'help'
  | 'save'
  | 'layers'
  | 'sidebar-swap'
  | 'focus-mode'
  ;

type PixelIcon = React.ComponentType<React.SVGProps<SVGSVGElement>>;

const PIXEL_ICONS: Record<string, PixelIcon> = {
  'effect.dithering': Grid3x3,
  'effect.glitching': Zap,
  'effect.curves': Chart,
  'effect.rgb': Brush,
  'effect.glow': Lightbulb,
  'effect.crt': Monitor,
  'effect.adjust': Settings2,
  'image.source': Image,
  plus: Plus,
  trash: Trash,
  'open-eye': Eye,
  'eye-off': EyeOff,
  close: Close,
  'row-img': Image,
  'image-actual': ImageNew,
  import: Download,
  export: Upload,
  'delete-con': Delete,
  sort: SortVertical,
  'auto-interpolate': GitMerge,
  'zoom-in': ZoomIn,
  'zoom-out': ZoomOut,
  'zoom-fit': Expand,
  'zoom-1x': Frame,
  'zoom-integer': Grid3x3,
  'color-lab': ColorsSwatch,
  preferences: SettingsCog,
  help: InfoBox,
  save: Save,
  layers: Files,
  'sidebar-swap': FlipHorizontal2,
  'focus-mode': Monitor,
};

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
  const Cmp = PIXEL_ICONS[name];
  if (Cmp) {
    return (
      <Cmp
        width={width}
        height={height}
        className={className}
        aria-hidden={alt ? undefined : true}
        role={alt ? 'img' : undefined}
        aria-label={alt || undefined}
        style={{
          display: 'block',
          flexShrink: 0,
          imageRendering: 'pixelated',
          ...style,
        }}
      />
    );
  }
  return <img src={name} width={width} height={height} alt={alt} style={style} className={className} />;
}

export default Icon;
