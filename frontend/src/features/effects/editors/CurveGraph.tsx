import { useCallback, useEffect, useRef, useState } from 'react';
import {
  addPoint,
  curveToPixel,
  curveToSvgPath,
  movePoint,
  nearestPointIndex,
  pixelToCurve,
  removePoint,
  type CurvePoint,
} from './curveMath';
import styles from './CurvesSettings.module.css';
import { bind } from '../../../shared/ui/cn';

const cn = bind(styles);

export const GRAPH_SIZE = 256;
export const GRAPH_PAD = 6;
const GRAPH_PAD_FRAC = GRAPH_PAD / GRAPH_SIZE;
const POINT_HALF = 4;
const DELETE_MARGIN_PX = 40;

const CHANNEL_STROKE: Record<string, string> = {
  All: '#000000',
  Red: '#cc0000',
  Green: '#007000',
  Blue: '#0000aa',
  Luminance: '#000000',
};

interface CurveGraphProps {
  curve: CurvePoint[];
  selectedIndex: number | null;
  channel: string;
  onChange: (curve: CurvePoint[]) => void;
  onSelect: (index: number | null) => void;
}

function gridLines(): number[] {
  return [0.25, 0.5, 0.75];
}

function CurveGraph({
  curve,
  selectedIndex,
  channel,
  onChange,
  onSelect,
}: CurveGraphProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const dragRef = useRef<{ index: number; pointerId: number } | null>(null);
  const onChangeRef = useRef(onChange);
  const onSelectRef = useRef(onSelect);
  const curveRef = useRef(curve);
  onChangeRef.current = onChange;
  onSelectRef.current = onSelect;

  const [display, setDisplay] = useState(curve);
  useEffect(() => {
    if (dragRef.current) return;
    setDisplay(curve);
    curveRef.current = curve;
  }, [curve]);

  const commit = (next: CurvePoint[]) => {
    curveRef.current = next;
    setDisplay(next);
    onChangeRef.current(next);
  };

  const stroke = CHANNEL_STROKE[channel] ?? CHANNEL_STROKE.All;
  const path = curveToSvgPath(display, GRAPH_SIZE, GRAPH_PAD);
  const inner = GRAPH_SIZE - GRAPH_PAD * 2;

  const toCurve = useCallback((clientX: number, clientY: number): [number, number] => {
    const svg = svgRef.current;
    if (!svg) return [0, 0];
    const rect = svg.getBoundingClientRect();
    return pixelToCurve(
      clientX - rect.left,
      clientY - rect.top,
      rect.width,
      rect.height,
      GRAPH_PAD_FRAC,
    );
  }, []);

  const hitRadius = useCallback((): number => {
    const svg = svgRef.current;
    const width = svg?.getBoundingClientRect().width ?? GRAPH_SIZE;
    const innerPx = Math.max(1, width * (1 - GRAPH_PAD_FRAC * 2));
    return 10 / innerPx;
  }, []);

  const outsideGraph = useCallback((clientX: number, clientY: number): boolean => {
    const svg = svgRef.current;
    if (!svg) return false;
    const rect = svg.getBoundingClientRect();
    return (
      clientX < rect.left - DELETE_MARGIN_PX
      || clientX > rect.right + DELETE_MARGIN_PX
      || clientY < rect.top - DELETE_MARGIN_PX
      || clientY > rect.bottom + DELETE_MARGIN_PX
    );
  }, []);

  const handlePointerDown = (e: React.PointerEvent<SVGSVGElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.focus();
    const [x, y] = toCurve(e.clientX, e.clientY);
    const hit = nearestPointIndex(curveRef.current, x, y, hitRadius());
    let index: number;
    if (hit !== null) {
      index = hit;
      onSelectRef.current(index);
    } else {
      const added = addPoint(curveRef.current, x, y);
      if (!added) return;
      curveRef.current = added.curve;
      index = added.index;
      commit(added.curve);
      onSelectRef.current(index);
    }
    dragRef.current = { index, pointerId: e.pointerId };
    if (typeof e.currentTarget.setPointerCapture === 'function') {
      e.currentTarget.setPointerCapture(e.pointerId);
    }
  };

  const handlePointerMove = (e: React.PointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== e.pointerId) return;
    const [x, y] = toCurve(e.clientX, e.clientY);
    const next = movePoint(curveRef.current, drag.index, x, y);
    commit(next);
  };

  const endDrag = (e: React.PointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== e.pointerId) return;
    dragRef.current = null;
    if (typeof e.currentTarget.hasPointerCapture === 'function'
      && e.currentTarget.hasPointerCapture(e.pointerId)
      && typeof e.currentTarget.releasePointerCapture === 'function') {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    if (outsideGraph(e.clientX, e.clientY)) {
      const removed = removePoint(curveRef.current, drag.index);
      if (removed) {
        commit(removed);
        onSelectRef.current(null);
      }
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<SVGSVGElement>) => {
    if (selectedIndex === null) return;
    const step = 1 / 255;
    const current = curveRef.current;
    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      const removed = removePoint(current, selectedIndex);
      if (removed) {
        commit(removed);
        onSelect(Math.min(selectedIndex, removed.length - 1));
      }
      return;
    }
    let dx = 0;
    let dy = 0;
    if (e.key === 'ArrowLeft') dx = -step;
    else if (e.key === 'ArrowRight') dx = step;
    else if (e.key === 'ArrowDown') dy = -step;
    else if (e.key === 'ArrowUp') dy = step;
    else return;
    e.preventDefault();
    const [x, y] = current[selectedIndex];
    commit(movePoint(current, selectedIndex, x + dx, y + dy));
  };

  return (
    <div className={cn('curve-graph-well')}>
      <svg
        ref={svgRef}
        className={cn('curve-graph')}
        viewBox={`0 0 ${GRAPH_SIZE} ${GRAPH_SIZE}`}
        role="application"
        aria-label="Tone curve"
        tabIndex={0}
        data-testid="curve-graph"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onKeyDown={handleKeyDown}
      >
        <rect
          x={GRAPH_PAD}
          y={GRAPH_PAD}
          width={inner}
          height={inner}
          className={cn('curve-graph-bg')}
        />
        {gridLines().map((t) => {
          const a = GRAPH_PAD + t * inner;
          return (
            <g key={t}>
              <line
                x1={GRAPH_PAD}
                y1={a}
                x2={GRAPH_PAD + inner}
                y2={a}
                className={cn('curve-grid')}
              />
              <line
                x1={a}
                y1={GRAPH_PAD}
                x2={a}
                y2={GRAPH_PAD + inner}
                className={cn('curve-grid')}
              />
            </g>
          );
        })}
        <line
          x1={GRAPH_PAD}
          y1={GRAPH_PAD + inner}
          x2={GRAPH_PAD + inner}
          y2={GRAPH_PAD}
          className={cn('curve-identity')}
        />
        <path
          d={path}
          className={cn('curve-stroke')}
          fill="none"
          stroke={stroke}
        />
        {display.map((point, i) => {
          const [px, py] = curveToPixel(point[0], point[1], GRAPH_SIZE, GRAPH_PAD);
          const selected = i === selectedIndex;
          return (
            <rect
              key={`${i}-${point[0]}-${point[1]}`}
              x={px - POINT_HALF}
              y={py - POINT_HALF}
              width={POINT_HALF * 2}
              height={POINT_HALF * 2}
              className={cn('curve-handle', selected && 'curve-handle-selected')}
              style={selected ? { fill: stroke, stroke } : { stroke }}
              data-testid={`curve-handle-${i}`}
            />
          );
        })}
      </svg>
    </div>
  );
}

export default CurveGraph;
