import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import PaletteManagerSection from '../PaletteManagerSection';
import type { BuiltinPaletteDto, PaletteDto } from '../../../shared/ipc';

const builtins: BuiltinPaletteDto[] = [
  {
    id: 'gameboy',
    name: 'Game Boy',
    colors: [
      [15, 56, 15],
      [48, 98, 48],
    ],
    color_count: 2,
  },
];

const saved: PaletteDto[] = [
  {
    id: 7,
    name: 'Extracted',
    colors: [[255, 0, 0]],
    hex_colors: ['#FF0000'],
    color_count: 1,
  },
];

describe('PaletteManagerSection', () => {
  it('lists New, built-in, and saved palettes in one menu', async () => {
    render(
      <PaletteManagerSection
        builtins={builtins}
        saved={saved}
        selectedPaletteId={null}
        onSelectNew={vi.fn()}
        onSelectSaved={vi.fn()}
        onSelectBuiltin={vi.fn()}
      />
    );
    expect(screen.getByText('New palette')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Open dropdown'));
    expect(screen.getByText('Built-in')).toBeInTheDocument();
    expect(screen.getByText('Saved')).toBeInTheDocument();
    expect(screen.getByText('Game Boy')).toBeInTheDocument();
    expect(screen.getByText('Extracted')).toBeInTheDocument();
  });

  it('selects a builtin template', () => {
    const onBuiltin = vi.fn();
    render(
      <PaletteManagerSection
        builtins={builtins}
        saved={saved}
        selectedPaletteId={null}
        onSelectNew={vi.fn()}
        onSelectSaved={vi.fn()}
        onSelectBuiltin={onBuiltin}
      />
    );
    fireEvent.click(screen.getByLabelText('Open dropdown'));
    fireEvent.click(screen.getByText('Game Boy'));
    expect(onBuiltin).toHaveBeenCalledWith('gameboy');
  });

  it('selects a saved palette', () => {
    const onSaved = vi.fn();
    render(
      <PaletteManagerSection
        builtins={builtins}
        saved={saved}
        selectedPaletteId={7}
        onSelectNew={vi.fn()}
        onSelectSaved={onSaved}
        onSelectBuiltin={vi.fn()}
      />
    );
    fireEvent.click(screen.getByLabelText('Open dropdown'));
    fireEvent.click(screen.getByRole('option', { name: /Extracted/ }));
    expect(onSaved).toHaveBeenCalledWith(7);
  });

  it('starts a new draft', () => {
    const onNew = vi.fn();
    render(
      <PaletteManagerSection
        builtins={builtins}
        saved={saved}
        selectedPaletteId={7}
        onSelectNew={onNew}
        onSelectSaved={vi.fn()}
        onSelectBuiltin={vi.fn()}
      />
    );
    fireEvent.click(screen.getByLabelText('Open dropdown'));
    fireEvent.click(screen.getByRole('option', { name: 'New palette' }));
    expect(onNew).toHaveBeenCalled();
  });
});
