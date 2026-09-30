import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { ShellContext } from '../../../app/shell/ShellContext';
import Tooltip from '../Tooltip';

describe('Tooltip', () => {
  it('shows the label near the cursor on hover', () => {
    render(
      <Tooltip label="Sort by brightness">
        <button type="button" aria-label="Sort by brightness">
          sort
        </button>
      </Tooltip>
    );
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByRole('button'), { clientX: 40, clientY: 20 });
    const tip = screen.getByRole('tooltip');
    expect(tip).toHaveTextContent('Sort by brightness');
    // jsdom size is 0 → sentinel 1×1; still places at cursor + OFFSET.
    expect(tip.style.left).toBe('54px');
    expect(tip.style.top).toBe('34px');
    expect(tip.style.visibility).toBe('visible');
  });

  it('hides on mouse leave', () => {
    render(
      <Tooltip label="Fit to view">
        <button type="button">fit</button>
      </Tooltip>
    );
    const host = screen.getByRole('button').parentElement!;
    fireEvent.mouseEnter(host, { clientX: 0, clientY: 0 });
    expect(screen.getByRole('tooltip')).toBeInTheDocument();
    fireEvent.mouseLeave(host);
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('clamps inside a dock panel instead of spilling past the right edge', () => {
    const panel = document.createElement('div');
    panel.setAttribute('data-dock-window', 'colorlab');
    Object.defineProperty(panel, 'getBoundingClientRect', {
      value: () => ({
        left: 0,
        top: 0,
        right: 200,
        bottom: 400,
        width: 200,
        height: 400,
        x: 0,
        y: 0,
        toJSON() {},
      }),
    });
    document.body.appendChild(panel);

    render(
      <Tooltip label="Auto interpolate — fill large brightness gaps">
        <button type="button">auto</button>
      </Tooltip>,
      { container: panel }
    );

    const button = screen.getByRole('button');

    act(() => {
      fireEvent.mouseEnter(button, { clientX: 180, clientY: 50 });
    });

    const tip = screen.getByRole('tooltip');
    Object.defineProperty(tip, 'offsetWidth', { configurable: true, value: 160 });
    Object.defineProperty(tip, 'offsetHeight', { configurable: true, value: 40 });

    act(() => {
      fireEvent.mouseMove(button, { clientX: 180, clientY: 50 });
    });

    const left = Number.parseFloat(tip.style.left);
    expect(left + 160).toBeLessThanOrEqual(200 - 6 + 0.5);
    panel.remove();
  });

  it('does not open when shell tooltips are disabled', () => {
    render(
      <ShellContext.Provider
        value={
          {
            tooltipsEnabled: false,
          } as React.ContextType<typeof ShellContext>
        }
      >
        <Tooltip label="Hidden">
          <button type="button">x</button>
        </Tooltip>
      </ShellContext.Provider>
    );
    fireEvent.mouseEnter(screen.getByRole('button'), { clientX: 10, clientY: 10 });
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });
});
