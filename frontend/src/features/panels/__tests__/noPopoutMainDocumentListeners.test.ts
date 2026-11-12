import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * W5: popout / floatable surfaces must not attach listeners to the main
 * `window` / `document`. Use ownerDocument / useOwnerDocument / capture target.
 */

const SRC = join(__dirname, '../../..');

/** Files that run inside (or for) flex-popout chrome / floated Preview. */
const POPOUT_FILES = [
  'features/panels/FlexPopoutChrome.tsx',
  'features/panels/useFlexPopoutJsDrag.ts',
  'components/PreviewWindow.tsx',
];

describe('W5 popout listener ownership', () => {
  it('forbids window./document. addEventListener in popout components', () => {
    const offenders: string[] = [];
    const bad = /\b(?:window|document)\.addEventListener\s*\(/;

    for (const rel of POPOUT_FILES) {
      const text = readFileSync(join(SRC, rel), 'utf8')
        .replace(/\/\*[\s\S]*?\*\//g, '')
        .replace(/\/\/.*$/gm, '');

      if (bad.test(text)) {
        offenders.push(rel);
      }
    }

    expect(offenders).toEqual([]);
  });
});
