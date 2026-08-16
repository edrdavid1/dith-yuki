import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import UpdateAvailableDialog from '../UpdateAvailableDialog';

describe('UpdateAvailableDialog', () => {
  it('Later does not call Install', () => {
    const onLater = vi.fn();
    const onInstall = vi.fn();
    render(
      <UpdateAvailableDialog
        isOpen
        version="0.2.1"
        notes="fixes"
        phase="prompt"
        downloaded={0}
        contentLength={null}
        error={null}
        onLater={onLater}
        onInstall={onInstall}
        onCancelDownload={vi.fn()}
      />
    );
    fireEvent.click(screen.getByText('Later'));
    expect(onLater).toHaveBeenCalledTimes(1);
    expect(onInstall).not.toHaveBeenCalled();
  });
});
