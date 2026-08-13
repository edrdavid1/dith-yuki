import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import PaletteManualEditor from '../PaletteManualEditor';
import { createColorEntry } from '../types';

const colors = [createColorEntry('#0f380f'), createColorEntry('#9bbc0f')];

describe('PaletteManualEditor selection', () => {
  it('highlights the selected row and reports clicks', () => {
    const onSelect = vi.fn();
    render(
      <PaletteManualEditor
        colors={colors}
        canAddColor
        selectedIndex={0}
        onSelect={onSelect}
        onChange={vi.fn()}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onOpenPicker={vi.fn()}
      />
    );
    const firstInput = screen.getByDisplayValue('#0f380f');
    expect(firstInput.closest('[aria-selected="true"]')).not.toBeNull();
    fireEvent.click(screen.getByDisplayValue('#9bbc0f'));
    expect(onSelect).toHaveBeenCalledWith(1);
  });

  it('preview-bar click selects the matching index', () => {
    const onSelect = vi.fn();
    render(
      <PaletteManualEditor
        colors={colors}
        canAddColor
        selectedIndex={null}
        onSelect={onSelect}
        onChange={vi.fn()}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onOpenPicker={vi.fn()}
      />
    );
    fireEvent.click(screen.getByTitle('#9bbc0f'));
    expect(onSelect).toHaveBeenCalledWith(1);
  });
});
