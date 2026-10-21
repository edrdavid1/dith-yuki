/**
 * Worker message shape with diagnostics off, and retry accounting when on.
 * diagnostic.md §10.1 and §10.8.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

describe('tileWorker diagnostics flag', () => {
  let posted: unknown[];
  let onmessage: ((e: MessageEvent) => Promise<void>) | null;

  beforeEach(() => {
    posted = [];
    onmessage = null;
    vi.stubGlobal('postMessage', (message: unknown) => {
      posted.push(message);
    });
    vi.stubGlobal('createImageBitmap', async () => ({ width: 256, height: 256, close() {} }));
    if (typeof globalThis.ImageData === 'undefined') {
      vi.stubGlobal(
        'ImageData',
        class {
          data: Uint8ClampedArray;
          width: number;
          height: number;
          constructor(data: Uint8ClampedArray, width: number, height: number) {
            this.data = data;
            this.width = width;
            this.height = height;
          }
        },
      );
    }
    const selfTarget = globalThis as unknown as { onmessage: unknown };
    Object.defineProperty(selfTarget, 'onmessage', {
      configurable: true,
      set(value) {
        onmessage = value;
      },
      get() {
        return onmessage;
      },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
    vi.resetModules();
  });

  async function load() {
    vi.resetModules();
    onmessage = null;
    await import('../tileWorker');
  }

  it('omits diag fields when the request does not ask for them', async () => {
    const bytes = 256 * 256 * 4;
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        status: 200,
        headers: { get: () => null },
        arrayBuffer: () => Promise.resolve(new ArrayBuffer(bytes)),
      }),
    );
    await load();
    await onmessage!({ data: { type: 'fetch-tile', level: 0, x: 1, y: 2, docId: 3 } } as MessageEvent);
    const msg = posted[0] as Record<string, unknown>;
    // H4 always attaches gen/quality/stale/pyramid/httpStatus; diag remains opt-in.
    expect(msg.diag).toBeUndefined();
    expect(msg.type).toBe('tile-decoded');
    expect(Object.keys(msg).sort()).toEqual(
      ['bitmap', 'docId', 'gen', 'httpStatus', 'key', 'pyramid', 'quality', 'rev', 'stale', 'type'].sort(),
    );
  });

  it('puts 202 sleep in retry_wait_ms and keeps it out of fetch_ms', async () => {
    vi.useFakeTimers();
    const bytes = 256 * 256 * 4;
    let calls = 0;
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation(() => {
        calls += 1;
        if (calls === 1) return Promise.resolve({ status: 202 });
        return Promise.resolve({
          status: 200,
          headers: { get: () => null },
          arrayBuffer: () => Promise.resolve(new ArrayBuffer(bytes)),
        });
      }),
    );
    await load();
    const pending = onmessage!({
      data: { type: 'fetch-tile', level: 0, x: 0, y: 0, docId: 1, diag: true },
    } as MessageEvent);
    await vi.advanceTimersByTimeAsync(50);
    await pending;
    const decoded = posted.find((m) => (m as { type?: string }).type === 'tile-decoded') as {
      diag: { retries: number; retry_wait_ms: number; fetch_ms: number };
    };
    expect(decoded.diag.retries).toBe(1);
    expect(decoded.diag.retry_wait_ms).toBe(50);
    expect(decoded.diag.fetch_ms).toBeLessThan(50);
  });
});
