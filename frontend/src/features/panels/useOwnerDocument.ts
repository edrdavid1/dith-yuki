/**
 * W5: resolve the Document that owns a popout (or main) DOM node.
 *
 * Popout portals run in a foreign browsing context. Listeners must attach to
 * `node.ownerDocument` / that document's `defaultView` — never assume the
 * main `window` / `document`.
 */

import { useLayoutEffect, useState, type RefObject } from 'react';

/** Document that owns `node`, falling back to the main document. */
export function ownerDocumentOf(node: Node | null | undefined): Document {
  return node?.ownerDocument ?? document;
}

/** defaultView for a node’s owner document (may be a popout Window). */
export function ownerWindowOf(node: Node | null | undefined): Window {
  return ownerDocumentOf(node).defaultView ?? window;
}

/**
 * Subscribe to the owner Document of a ref. Updates when the ref mounts into
 * a (possibly foreign) document — e.g. FlexLayout portal into flex-popout-*.
 */
export function useOwnerDocument(
  ref: RefObject<Node | null>,
): Document {
  const [doc, setDoc] = useState<Document>(() =>
    ownerDocumentOf(ref.current),
  );

  useLayoutEffect(() => {
    const next = ownerDocumentOf(ref.current);
    setDoc((prev) => (prev === next ? prev : next));
  });

  return doc;
}

/** Window for `useOwnerDocument(ref)`. */
export function useOwnerWindow(ref: RefObject<Node | null>): Window {
  const doc = useOwnerDocument(ref);
  return doc.defaultView ?? window;
}
