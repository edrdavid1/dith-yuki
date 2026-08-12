/**
 * Bug Condition Exploration Test — Sequential Tile Fetch
 *
 * **Validates: Requirements 1.1, 1.2, 1.3**
 *
 * This test verifies that for a batch of tiles (length > 1), all fetch requests
 * start concurrently (within EPSILON of each other).
 *
 * On UNFIXED code, this test FAILS because tileWorker.ts uses `for...await`
 * which serializes fetch calls sequentially.
 *
 * DO NOT fix the code or the test when it fails — failure confirms the bug exists.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import * as fc from 'fast-check';

// We can't run an actual Web Worker in vitest/jsdom, so we import the worker
// module directly and simulate the onmessage handler.
// The worker sets `self.onmessage` — we'll evaluate its logic by importing
// and calling the handler directly.

describe('Bug Condition: Sequential Tile Fetch', () => {
  // Track fetch call start timestamps
  let fetchStartTimes: number[] = [];
  let postMessages: unknown[] = [];
  const ARTIFICIAL_DELAY_MS = 50; // Delay per fetch to make sequentiality observable
  const EPSILON_MS = 20; // Concurrent fetches should start within this window

  beforeEach(() => {
    fetchStartTimes = [];
    postMessages = [];

    // Mock global postMessage (worker context)
    vi.stubGlobal('postMessage', (...args: unknown[]) => {
      postMessages.push(args[0]);
    });

    // Mock createImageBitmap
    vi.stubGlobal('createImageBitmap', async () => {
      return {} as ImageBitmap;
    });

    // Mock fetch to record start timestamps with artificial delay
    vi.stubGlobal('fetch', async (_url: string) => {
      fetchStartTimes.push(performance.now());
      // Artificial delay to make sequential behavior observable
      await new Promise(resolve => setTimeout(resolve, ARTIFICIAL_DELAY_MS));
      // Return a successful response with correct-sized RGBA8 buffer
      const buffer = new ArrayBuffer(256 * 256 * 4);
      return {
        status: 200,
        arrayBuffer: async () => buffer,
        text: async () => '',
      };
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it('property: all tile fetches in a batch start concurrently (within EPSILON)', async () => {
    await fc.assert(
      fc.asyncProperty(
        fc.array(
          fc.record({
            level: fc.nat({ max: 3 }),
            x: fc.nat({ max: 15 }),
            y: fc.nat({ max: 15 }),
          }),
          { minLength: 2, maxLength: 10 }
        ),
        async (tiles) => {
          // Reset state for each property run
          fetchStartTimes = [];
          postMessages = [];

          // Re-import the worker module fresh to get the onmessage handler
          // We need to reset modules to get a fresh import each time
          vi.resetModules();

          // Dynamically import the worker — it registers self.onmessage on load
          const selfObj = globalThis as unknown as { onmessage?: (e: MessageEvent) => Promise<void> };
          await import('../../workers/tileWorker');

          const handler = selfObj.onmessage;
          expect(handler).toBeDefined();

          // Simulate the 'request-tiles' message
          const message = {
            data: {
              type: 'request-tiles',
              tiles,
              docId: 1,
            },
          } as MessageEvent;

          await handler!(message);

          // All fetches should have been called
          expect(fetchStartTimes.length).toBe(tiles.length);

          // Bug condition assertion: all fetch start times should be within EPSILON
          // On UNFIXED code, they will be staggered by ARTIFICIAL_DELAY_MS
          const minTime = Math.min(...fetchStartTimes);
          const maxTime = Math.max(...fetchStartTimes);
          const spread = maxTime - minTime;

          expect(spread).toBeLessThan(EPSILON_MS);
        }
      ),
      { numRuns: 20 } // Enough to confirm the pattern
    );
  });
});
