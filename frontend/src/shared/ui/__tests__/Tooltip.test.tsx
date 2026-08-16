import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
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
    expect(tip).toHaveStyle({ left: '54px', top: '34px' });
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
});
