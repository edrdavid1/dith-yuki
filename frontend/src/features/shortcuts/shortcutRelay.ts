/**
 * Relay keyboard events from FlexLayout popout documents into the shortcut
 * engine (which listens on the main window). Portaled React still runs in the
 * main JS realm, but keydown targets the popout `Window`.
 */

type ShortcutHandler = (e: KeyboardEvent) => void;

const handlers = new Set<ShortcutHandler>();

export function subscribeShortcutRelay(handler: ShortcutHandler): () => void {
  handlers.add(handler);
  return () => {
    handlers.delete(handler);
  };
}

export function relayShortcutEvent(e: KeyboardEvent): void {
  for (const handler of handlers) {
    handler(e);
  }
}
