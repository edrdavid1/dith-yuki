/**
 * Preservation property tests for tileWorker.ts
 *
 * These tests verify behavior that MUST NOT change after the bugfix:
 * - Single `fetch-tile` messages produce exactly one response with the correct shape
 * - Response type matches fetch response status (200 → tile-decoded, 202 → tile-pending, error → tile-error)
 *
 * **Validates: Requirements 3.1, 3.2, 3.3, 3.6**
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import * as fc from 'fast-check';

// We can't run a real Web Worker in vitest, so we simulate the worker environment
// by mocking globals and importing the module which registers self.onmessage.

describe('tileWorker preservation: single fetch-tile behavior', () => {
  let postedMessages: Array<{ message: unknown; transfer?: Transferable[] }>;
  let originalFetch: typeof globalThis.fetch;
  let originalCreateImageBitmap: typeof globalThis.createImageBitmap;
  let onmessageHandler: ((e: MessageEvent) => Promise<void>) | null;

  beforeEach(() => {
    postedMessages = [];

    // Mock postMessage at global level (worker scope)
    (globalThis as any).postMessage = (message: unknown, transferOrOptions?: Transferable[] | StructuredSerializeOptions) => {
      const transfer = Array.isArray(transferOrOptions) ? transferOrOptions : undefined;
      postedMessages.push({ message, transfer });
    };

    // Save originals
    originalFetch = globalThis.fetch;
    originalCreateImageBitmap = globalThis.createImageBitmap;

    // Mock createImageBitmap to return a fake ImageBitmap
    const fakeBitmap = { width: 256, height: 256, close: () => {} } as unknown as ImageBitmap;
    (globalThis as any).createImageBitmap = vi.fn().mockResolvedValue(fakeBitmap);

    // Mock ImageData constructor if not available in jsdom
    if (typeof globalThis.ImageData === 'undefined') {
      (globalThis as any).ImageData = class MockImageData {
        data: Uint8ClampedArray;
        width: number;
        height: number;
        constructor(data: Uint8ClampedArray, width: number, height: number) {
          this.data = data;
          this.width = width;
          this.height = height;
        }
      };
    }

    // We'll set up the onmessage handler by re-importing the module
    onmessageHandler = null;

    // Mock self.onmessage setter to capture the handler
    const originalSelf = globalThis.self;
    Object.defineProperty(globalThis, 'self', {
      value: new Proxy(originalSelf || globalThis, {
        set(target, prop, value) {
          if (prop === 'onmessage') {
            onmessageHandler = value;
            return true;
          }
          (target as any)[prop] = value;
          return true;
        },
        get(target, prop) {
          if (prop === 'onmessage') {
            return onmessageHandler;
          }
          return (target as any)[prop];
        },
      }),
      writable: true,
      configurable: true,
    });
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    (globalThis as any).createImageBitmap = originalCreateImageBitmap;
    vi.restoreAllMocks();
  });

  /**
   * Helper: load the tileWorker module fresh (captures onmessage handler)
   */
  async function loadWorker() {
    // Dynamic import with cache-busting to get a fresh module each time
    const modulePath = '../tileWorker';
    // Clear module from vitest cache
    vi.resetModules();
    await import(modulePath);
  }

  /**
   * Helper: simulate sending a fetch-tile message to the worker
   */
  async function sendFetchTile(level: number, x: number, y: number, docId: number = 1) {
    if (!onmessageHandler) {
      throw new Error('Worker onmessage handler not registered');
    }
    const event = { data: { type: 'fetch-tile', level, x, y, docId } } as MessageEvent;
    await onmessageHandler(event);
  }

  it('property: single fetch-tile with 200 response produces tile-decoded with correct key', async () => {
    const TILE_BYTE_LENGTH = 256 * 256 * 4; // 262144

    await fc.assert(
      fc.asyncProperty(
        fc.record({
          level: fc.integer({ min: 0, max: 3 }),
          x: fc.integer({ min: 0, max: 15 }),
          y: fc.integer({ min: 0, max: 15 }),
        }),
        async (tile) => {
          // Setup mock fetch to return 200 with correct byte length
          globalThis.fetch = vi.fn().mockResolvedValue({
            status: 200,
            arrayBuffer: () => Promise.resolve(new ArrayBuffer(TILE_BYTE_LENGTH)),
          } as unknown as Response);

          postedMessages = [];
          await loadWorker();
          await sendFetchTile(tile.level, tile.x, tile.y);

          // Exactly one message posted
          expect(postedMessages.length).toBe(1);

          const msg = postedMessages[0].message as any;
          expect(msg.type).toBe('tile-decoded');
          expect(msg.key).toBe(`${tile.level}/${tile.x}/${tile.y}`);
          expect(msg.bitmap).toBeDefined();

          // Transfer array should contain the bitmap
          expect(postedMessages[0].transfer).toBeDefined();
          expect(postedMessages[0].transfer!.length).toBe(1);
        },
      ),
      { numRuns: 50 },
    );
  });

  it('property: single fetch-tile with 202 response produces tile-pending with correct key', async () => {
    vi.useFakeTimers();
    try {
      await fc.assert(
        fc.asyncProperty(
          fc.record({
            level: fc.integer({ min: 0, max: 3 }),
            x: fc.integer({ min: 0, max: 15 }),
            y: fc.integer({ min: 0, max: 15 }),
          }),
          async (tile) => {
            // Setup mock fetch to always return 202 (pending) for all attempts including retries
            globalThis.fetch = vi.fn().mockResolvedValue({
              status: 202,
            } as unknown as Response);

            postedMessages = [];
            onmessageHandler = null;
            await loadWorker();

            // Send the fetch-tile message (don't await — it will block on setTimeout retries)
            const promise = sendFetchTile(tile.level, tile.x, tile.y);

            // Advance timers to flush all retry delays (50+100+200+400+800 = 1550ms)
            await vi.advanceTimersByTimeAsync(2000);
            await promise;

            // First message posted should be tile-pending
            expect(postedMessages.length).toBeGreaterThanOrEqual(1);

            const msg = postedMessages[0].message as any;
            expect(msg.type).toBe('tile-pending');
            expect(msg.key).toBe(`${tile.level}/${tile.x}/${tile.y}`);
          },
        ),
        { numRuns: 20 },
      );
    } finally {
      vi.useRealTimers();
    }
  }, 30000);

  it('property: single fetch-tile with fetch error produces tile-error with correct key and message', async () => {
    await fc.assert(
      fc.asyncProperty(
        fc.record({
          level: fc.integer({ min: 0, max: 3 }),
          x: fc.integer({ min: 0, max: 15 }),
          y: fc.integer({ min: 0, max: 15 }),
        }),
        fc.string({ minLength: 1, maxLength: 50 }),
        async (tile, errorMsg) => {
          // Setup mock fetch to throw an error
          globalThis.fetch = vi.fn().mockRejectedValue(new Error(errorMsg));

          postedMessages = [];
          onmessageHandler = null;
          await loadWorker();
          await sendFetchTile(tile.level, tile.x, tile.y);

          // At least one message posted — first should be tile-error
          expect(postedMessages.length).toBeGreaterThanOrEqual(1);

          const msg = postedMessages[0].message as any;
          expect(msg.type).toBe('tile-error');
          expect(msg.key).toBe(`${tile.level}/${tile.x}/${tile.y}`);
          expect(msg.error).toBeDefined();
          expect(typeof msg.error).toBe('string');
          expect(msg.error.length).toBeGreaterThan(0);
        },
      ),
      { numRuns: 50 },
    );
  });
});
