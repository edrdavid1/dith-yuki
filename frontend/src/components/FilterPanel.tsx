import type { FilterInfo } from '../types';
import DitherParams from './filters/DitherParams';
import LevelsParams from './filters/LevelsParams';
import GlitchParams from './filters/GlitchParams';
import CurvesParams from './filters/CurvesParams';

interface FilterPanelProps {
  filter: FilterInfo;
  onUpdate: (filterId: string, params: Record<string, unknown>) => void;
}

function FilterPanel({ filter, onUpdate }: FilterPanelProps) {
  const handleChange = (params: Record<string, unknown>) => {
    onUpdate(filter.id, params);
  };

  switch (filter.kind) {
    case 'Dither':
      return (
        <DitherParams
          algorithm={(filter.params as any).algorithm ?? 'FloydSteinberg'}
          colorDepth={(filter.params as any).color_depth ?? 4}
          onChange={handleChange}
        />
      );
    case 'Curves':
      return (
        <CurvesParams
          curve={(filter.params as any).curve ?? [[0, 0], [1, 1]]}
          channel={(filter.params as any).channel ?? 'All'}
          onChange={handleChange}
        />
      );
    case 'Levels':
      return (
        <LevelsParams
          inputBlack={(filter.params as any).input_black ?? 0}
          inputWhite={(filter.params as any).input_white ?? 1}
          gamma={(filter.params as any).gamma ?? 1}
          outputBlack={(filter.params as any).output_black ?? 0}
          outputWhite={(filter.params as any).output_white ?? 1}
          onChange={handleChange}
        />
      );
    case 'Glitch':
      return (
        <GlitchParams
          glitchType={(filter.params as any).glitch_type ?? 'RGBShift'}
          intensity={(filter.params as any).intensity ?? 0.5}
          seed={(filter.params as any).seed ?? 42}
          onChange={handleChange}
        />
      );
    default:
      return <div>Unknown filter type</div>;
  }
}

export default FilterPanel;
