import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import ShareCopyDialog, { shareCopyDefaultOptions } from '../ShareCopyDialog';

describe('ShareCopyDialog', () => {
  it('defaults match SPEC §11', () => {
    const d = shareCopyDefaultOptions();
    expect(d.stripMetadata).toBe(true);
    expect(d.includeOriginalImages).toBe(false);
    expect(d.includeAuthor).toBe(false);
    expect(d.compact).toBe(false);
  });

  it('renders checkboxes with SPEC defaults checked/unchecked', () => {
    render(<ShareCopyDialog isOpen onExport={() => {}} onCancel={() => {}} />);
    expect(screen.getByTestId('share-copy-strip-metadata')).toBeChecked();
    expect(screen.getByTestId('share-copy-include-originals')).not.toBeChecked();
    expect(screen.getByTestId('share-copy-include-author')).not.toBeChecked();
    expect(screen.getByTestId('share-copy-compact')).not.toBeChecked();
  });
});
