import { render, screen } from '@testing-library/react';
import HelpDialog from '../HelpDialog';

describe('HelpDialog channel', () => {
  it('shows Report a bug only on the direct channel', () => {
    render(
      <HelpDialog isOpen version="2.0.6-beta" onClose={() => {}} />,
    );
    const button = screen.queryByRole('button', { name: 'Report a bug' });
    if (__CHANNEL__ === 'direct') {
      expect(button).not.toBeNull();
    } else {
      expect(button).toBeNull();
    }
    expect(screen.getByRole('dialog')).toBeTruthy();
  });
});
