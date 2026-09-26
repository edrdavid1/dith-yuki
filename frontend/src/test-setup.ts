import '@testing-library/jest-dom';
import { beforeEach, vi } from 'vitest';

// Node 22+ may leave `localStorage` undefined under jsdom; stub a minimal store.
const localStore = new Map<string, string>();
if (typeof globalThis.localStorage === 'undefined' || globalThis.localStorage == null) {
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => (localStore.has(key) ? localStore.get(key)! : null),
    setItem: (key: string, value: string) => {
      localStore.set(key, String(value));
    },
    removeItem: (key: string) => {
      localStore.delete(key);
    },
    clear: () => {
      localStore.clear();
    },
    key: (index: number) => Array.from(localStore.keys())[index] ?? null,
    get length() {
      return localStore.size;
    },
  });
}

beforeEach(() => {
  try {
    globalThis.localStorage?.clear();
  } catch {
    localStore.clear();
  }
});

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(),
  emitTo: vi.fn(),
}));

