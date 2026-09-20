import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import DropdownMenu from '../common/DropdownMenu';

describe('DropdownMenu portal', () => {
  it('mounts the list into ownerDocument.body, not #overlay-portal', () => {
    const overlay = document.createElement('div');
    overlay.id = 'overlay-portal';
    document.body.appendChild(overlay);

    render(
      <DropdownMenu
        value="a"
        options={[
          { value: 'a', label: 'Alpha' },
          { value: 'b', label: 'Beta' },
        ]}
        onSelect={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByLabelText('Open dropdown'));
    const list = screen.getByRole('listbox');
    expect(overlay.contains(list)).toBe(false);
    expect(list.closest('body')).toBe(document.body);

    overlay.remove();
  });

  it('closes on wheel outside the menu so it stays viewport-pinned', () => {
    render(
      <DropdownMenu
        value="a"
        options={[
          { value: 'a', label: 'Alpha' },
          { value: 'b', label: 'Beta' },
        ]}
        onSelect={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByLabelText('Open dropdown'));
    expect(screen.getByRole('listbox')).toBeInTheDocument();
    fireEvent.wheel(window, { deltaY: 40 });
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });
});
