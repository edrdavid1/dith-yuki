import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import WindowTitlebar from '../WindowTitlebar';

describe('WindowTitlebar menu', () => {
  it('renders menu on square click and clamps to window bounds', () => {
    window.innerWidth = 800;
    window.innerHeight = 600;

    const onMove = vi.fn();
    const onPop = vi.fn();

    render(
      <WindowTitlebar
        title="Layers"
        dockSide="left"
        onMoveToSide={onMove}
        onPopOut={onPop}
      />
    );

    const buttons = screen.getAllByLabelText('Panel menu');
    const squareBtn = buttons[0];
    expect(squareBtn).toBeDefined();

    vi.spyOn(squareBtn, 'getBoundingClientRect').mockReturnValue({
      left: 750,
      top: 10,
      right: 764,
      bottom: 24,
      width: 14,
      height: 14,
      x: 750,
      y: 10,
      toJSON: () => ({}),
    });

    fireEvent.click(squareBtn);

    const moveOption = screen.getByText('Move to right sidebar');
    const popOption = screen.getByText('Open in separate window');

    expect(moveOption).toBeDefined();
    expect(popOption).toBeDefined();

    const menuEl = moveOption.closest('[role="menu"]') as HTMLElement;
    expect(menuEl).toBeDefined();

    fireEvent.click(moveOption);
    expect(onMove).toHaveBeenCalledWith('right');
  });
});
