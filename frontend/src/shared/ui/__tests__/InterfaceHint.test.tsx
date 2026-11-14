import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ShellContext } from '../../../app/shell/ShellContext';
import InterfaceHint from '../InterfaceHint';

function shell(tooltipsEnabled: boolean) {
  return { tooltipsEnabled } as React.ContextType<typeof ShellContext>;
}

describe('InterfaceHint', () => {
  it('shows inline copy when tooltips are on', () => {
    render(
      <ShellContext.Provider value={shell(true)}>
        <InterfaceHint>
          <p>Palette is controlled in Color Lab</p>
        </InterfaceHint>
      </ShellContext.Provider>
    );
    expect(screen.getByText('Palette is controlled in Color Lab')).toBeInTheDocument();
  });

  it('hides inline copy when tooltips are off', () => {
    render(
      <ShellContext.Provider value={shell(false)}>
        <InterfaceHint>
          <p>Palette is controlled in Color Lab</p>
        </InterfaceHint>
        <button type="button">Tone Steps</button>
      </ShellContext.Provider>
    );
    expect(screen.queryByText('Palette is controlled in Color Lab')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Tone Steps' })).toBeInTheDocument();
  });

  it('stays visible when no shell is mounted', () => {
    render(
      <InterfaceHint>
        <p>Palette is controlled in Color Lab</p>
      </InterfaceHint>
    );
    expect(screen.getByText('Palette is controlled in Color Lab')).toBeInTheDocument();
  });
});
