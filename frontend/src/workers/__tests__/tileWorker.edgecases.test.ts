/**
 * Edge case tests for tileWorker.ts batch handling
 *
 * Verifies correct behavior for boundary inputs after the parallel fetch fix:
 * 1. Empty batch: `request-tiles` with `tiles: []` resolves immediately, no messages posted
 * 2. Single tile in batch: functionally equivalent to single `await`
 * 3. Very large batch: 100 tiles all resolve without resource exhaustion
 *
 * **Validates: Requirements 2.1, 2.2**
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

describe('tileWorker edge cases: request-tiles boundary inputs', () => {
  let postedMessages: Array<{ message: unknown; transfer?: Transferable[] }>;
  let fetchCallCount: number;
  let onmessageHandler: ((e: MessageEvent) => Promise<void>) | null;

  const TILE_BYTE_LENGTH = 256 * 256 * 4; // 262144 bytes RGBA8

  beforeEach(() => {
    postedMessages = [];
    fetchCallCount = 0;
    onmessageHandler = null;

    // Mock postMessage (worker scope)
    vi.stubGlobal('postMessage', (message: unknown, transferOrOptions?: Transferable[] | StructuredSerializeOptions) => {
      const transfer = Array.isArray(transferOrOptions) ? transferOrOptions : undefined;
      postedMessages.push({ message, transfer });
    });

    // Mock createImageBitmap
    vi.stubGlobal('createImageBitmap', async () => {
      return { width: 256, height: 256, close: () => {} } as unknown as ImageBitmap;
    });

    // Mock ImageData if not available in jsdom
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

    // Mock fetch — returns 200 with correct buffer size
    vi.stubGlobal('fetch', async (_url: string) => {
      fetchCallCount++;
      return {
        status: 200,
        arrayBuffer: async () => new ArrayBuffer(TILE_BYTE_LENGTH),
        text: async () => '',
      };
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  /**
   * Helper: load worker module fresh and capture onmessage handler
   */
  async function loadWorkerAndGetHandler(): Promise<(e: MessageEvent) => Promise<void>> {
    vi.resetModules();

    const selfObj = globalThis as unknown as { onmessage?: (e: MessageEvent) => Promise<void> };
    await import('../../workers/tileWorker');

    const handler = selfObj.onmessage;
    if (!handler) throw new Error('Worker onmessage handler not registered');
    return handler;
  }

  it('empty batch: request-tiles with tiles=[] resolves immediately with no messages posted', async () => {
    const handler = await loadWorkerAndGetHandler();

    const message = {
      data: {
        type: 'request-tiles',
        tiles: [],
        docId: 1,
      },
    } as MessageEvent;

    await handler(message);

    // No fetch calls should have been made
    expect(fetchCallCount).toBe(0);
    // No messages should have been posted
    expect(postedMessages.length).toBe(0);
  });

  it('single tile in batch: request-tiles with 1 tile produces exactly one tile-decoded message', async () => {
    const handler = await loadWorkerAndGetHandler();

    const message = {
      data: {
        type: 'request-tiles',
        tiles: [{ level: 0, x: 3, y: 7 }],
        docId: 1,
      },
    } as MessageEvent;

    await handler(message);

    // Exactly one fetch call
    expect(fetchCallCount).toBe(1);
    // Exactly one message posted
    expect(postedMessages.length).toBe(1);

    const msg = postedMessages[0].message as any;
    expect(msg.type).toBe('tile-decoded');
    expect(msg.key).toBe('0/3/7');
    expect(msg.bitmap).toBeDefined();
  });

  it('very large batch: request-tiles with 100 tiles resolves without resource exhaustion', async () => {
    const handler = await loadWorkerAndGetHandler();

    // Generate 100 tile requests (10x10 grid)
    const tiles = Array.from({ length: 100 }, (_, i) => ({
      level: 0,
      x: i % 10,
      y: Math.floor(i / 10),
    }));

    const message = {
      data: {
        type: 'request-tiles',
        tiles,
        docId: 1,
      },
    } as MessageEvent;

    await handler(message);

    // All 100 fetches should complete
    expect(fetchCallCount).toBe(100);
    // All 100 messages should be posted
    expect(postedMessages.length).toBe(100);

    // All messages should be tile-decoded
    for (const posted of postedMessages) {
      const msg = posted.message as any;
      expect(msg.type).toBe('tile-decoded');
      expect(msg.key).toBeDefined();
      expect(msg.bitmap).toBeDefined();
    }
  });
});
