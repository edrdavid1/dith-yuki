import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import CurvesSettings from '../CurvesSettings';
import { GRAPH_PAD, GRAPH_SIZE } from '../CurveGraph';
import { pixelToCurve } from '../curveMath';

function mockGraphBox(svg: HTMLElement) {
  vi.spyOn(svg, 'getBoundingClientRect').mockReturnValue({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    bottom: GRAPH_SIZE,
    right: GRAPH_SIZE,
    width: GRAPH_SIZE,
    height: GRAPH_SIZE,
    toJSON() {
      return {};
    },
  });
}

function clientForCurve(x: number, y: number): { clientX: number; clientY: number } {
  const inner = GRAPH_SIZE - GRAPH_PAD * 2;
  return {
    clientX: GRAPH_PAD + x * inner,
    clientY: GRAPH_PAD + (1 - y) * inner,
  };
}

describe('CurvesSettings', () => {
  it('renders tagged engine params as a bent curve, not identity', () => {
    render(
      <CurvesSettings
        params={{
          Curves: {
            curve: [[0, 0], [0.5, 0.8], [1, 1]],
            channel: 'Red',
          },
        }}
        onUpdate={vi.fn()}
      />,
    );
    expect(screen.getByTestId('curve-handle-0')).toBeInTheDocument();
    expect(screen.getByTestId('curve-handle-1')).toBeInTheDocument();
    expect(screen.getByTestId('curve-handle-2')).toBeInTheDocument();
    expect(screen.getByText('Red')).toBeInTheDocument();
    expect(screen.getByLabelText('Input')).toHaveValue('0');
    expect(screen.getByLabelText('Output')).toHaveValue('0');
  });

  it('renders the graph, channel, and 0–255 Input/Output fields', () => {
    render(
      <CurvesSettings
        params={{ curve: [[0, 0], [1, 1]], channel: 'All' }}
        onUpdate={vi.fn()}
      />,
    );
    expect(screen.getByTestId('curve-graph')).toBeInTheDocument();
    expect(screen.getByText('Channel')).toBeInTheDocument();
    expect(screen.getByText('All')).toBeInTheDocument();
    expect(screen.getByLabelText('Input')).toHaveValue('0');
    expect(screen.getByLabelText('Output')).toHaveValue('0');
    expect(screen.getByText('Reset')).toBeInTheDocument();
  });

  it('adds a point on empty-graph click', () => {
    const onUpdate = vi.fn();
    render(
      <CurvesSettings
        params={{ curve: [[0, 0], [1, 1]], channel: 'All' }}
        onUpdate={onUpdate}
      />,
    );
    const svg = screen.getByTestId('curve-graph');
    mockGraphBox(svg);
    const { clientX, clientY } = clientForCurve(0.5, 0.75);
    fireEvent.pointerDown(svg, { button: 0, pointerId: 1, clientX, clientY });
    expect(onUpdate).toHaveBeenCalled();
    const payload = onUpdate.mock.calls[0][0] as { curve: [number, number][] };
    expect(payload.curve).toHaveLength(3);
    expect(payload.curve[1][0]).toBeCloseTo(0.5, 2);
    expect(payload.curve[1][1]).toBeCloseTo(0.75, 2);
  });

  it('drags an existing point', () => {
    const onUpdate = vi.fn();
    render(
      <CurvesSettings
        params={{ curve: [[0, 0], [1, 1]], channel: 'All' }}
        onUpdate={onUpdate}
      />,
    );
    const svg = screen.getByTestId('curve-graph');
    mockGraphBox(svg);
    fireEvent.pointerDown(svg, { button: 0, pointerId: 1, ...clientForCurve(0, 0) });
    fireEvent.pointerMove(svg, { pointerId: 1, ...clientForCurve(0, 0.4) });
    expect(onUpdate).toHaveBeenCalled();
    const last = onUpdate.mock.calls[onUpdate.mock.calls.length - 1][0] as {
      curve: [number, number][];
    };
    expect(last.curve[0][1]).toBeCloseTo(0.4, 2);
    expect(last.curve).toHaveLength(2);
  });

  it('updates Output via the numeric field (0–255)', () => {
    const onUpdate = vi.fn();
    render(
      <CurvesSettings
        params={{ curve: [[0, 0], [1, 1]], channel: 'All' }}
        onUpdate={onUpdate}
      />,
    );
    const output = screen.getByLabelText('Output');
    fireEvent.change(output, { target: { value: '64' } });
    fireEvent.blur(output);
    expect(onUpdate).toHaveBeenCalled();
    const payload = onUpdate.mock.calls[0][0] as { curve: [number, number][] };
    expect(payload.curve[0][0]).toBe(0);
    expect(payload.curve[0][1]).toBeCloseTo(64 / 255, 5);
  });

  it('resets to an identity curve', () => {
    const onUpdate = vi.fn();
    render(
      <CurvesSettings
        params={{ curve: [[0, 0.2], [0.5, 0.8], [1, 1]], channel: 'All' }}
        onUpdate={onUpdate}
      />,
    );
    fireEvent.click(screen.getByText('Reset'));
    expect(onUpdate).toHaveBeenCalledWith(
      expect.objectContaining({ curve: [[0, 0], [1, 1]] }),
    );
  });

  it('changes channel from the dropdown', () => {
    const onUpdate = vi.fn();
    render(
      <CurvesSettings
        params={{ curve: [[0, 0], [1, 1]], channel: 'All' }}
        onUpdate={onUpdate}
      />,
    );
    fireEvent.click(screen.getByLabelText('Open dropdown'));
    fireEvent.click(screen.getByRole('option', { name: 'Red' }));
    expect(onUpdate).toHaveBeenCalledWith(expect.objectContaining({ channel: 'Red' }));
  });

  it('deletes the selected extra point with Backspace', () => {
    const onUpdate = vi.fn();
    render(
      <CurvesSettings
        params={{ curve: [[0, 0], [0.5, 0.5], [1, 1]], channel: 'All' }}
        onUpdate={onUpdate}
      />,
    );
    const svg = screen.getByTestId('curve-graph');
    mockGraphBox(svg);
    fireEvent.pointerDown(svg, { button: 0, pointerId: 1, ...clientForCurve(0.5, 0.5) });
    fireEvent.pointerUp(svg, { pointerId: 1, ...clientForCurve(0.5, 0.5) });
    fireEvent.keyDown(svg, { key: 'Backspace' });
    expect(onUpdate).toHaveBeenCalled();
    const last = onUpdate.mock.calls[onUpdate.mock.calls.length - 1][0] as {
      curve: [number, number][];
    };
    expect(last.curve).toEqual([[0, 0], [1, 1]]);
  });
});

describe('pixel mapping used by the graph', () => {
  it('round-trips the click helper through pixelToCurve', () => {
    const { clientX, clientY } = clientForCurve(0.5, 0.75);
    const [x, y] = pixelToCurve(
      clientX,
      clientY,
      GRAPH_SIZE,
      GRAPH_SIZE,
      GRAPH_PAD / GRAPH_SIZE,
    );
    expect(x).toBeCloseTo(0.5, 5);
    expect(y).toBeCloseTo(0.75, 5);
  });
});
