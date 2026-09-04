/**
 * Patched FlexLayout FloatingWindow for Tauri / WKWebView (CommonJS).
 * Copied into node_modules/flexlayout-react/lib/view/FloatingWindow.js on postinstall.
 *
 * - Do not use popout beforeunload → onCloseWindow (fires on occlusion → blank portal).
 * - Reuse named Window across React remounts (owner refcount).
 * - On real unmount (unfloat / dock), close the OS window after a short grace period
 *   so remounts don't leave a dead gray rectangle.
 */

"use strict";

Object.defineProperty(exports, "__esModule", { value: true });
exports.FloatingWindow = void 0;

const React = require("react");
const react_dom_1 = require("react-dom");
const Rect_1 = require("../Rect");
const Types_1 = require("../Types");

const CONTENT_CLASS = Types_1.CLASSES.FLEXLAYOUT__FLOATING_WINDOW_CONTENT;

/**
 * @typedef {{ win: Window, owners: number, closeTimer: ReturnType<typeof setTimeout> | null }} RegistryEntry
 * @type {Map<string, RegistryEntry>}
 */
const windowRegistry = new Map();

function copyDomStyles(destDoc) {
  if (!destDoc || !destDoc.head) return;
  const marker = "data-flex-dom-styles";
  destDoc.head.querySelectorAll("[" + marker + "]").forEach((n) => n.remove());

  const srcHead = window.document.head;
  const nodes = srcHead.querySelectorAll('style, link[rel="stylesheet"]');
  nodes.forEach((node) => {
    const clone = node.cloneNode(true);
    clone.setAttribute(marker, "1");
    if (clone.tagName === "LINK" && clone.getAttribute("href")) {
      try {
        clone.setAttribute("href", new URL(clone.getAttribute("href"), window.location.href).href);
      } catch (_e) {
        /* keep */
      }
    }
    destDoc.head.appendChild(clone);
  });
}

function ensureContentRoot(popoutDocument, title) {
  popoutDocument.title = title;
  let root = popoutDocument.querySelector("." + CONTENT_CLASS);
  if (!root) {
    root = popoutDocument.createElement("div");
    root.className = CONTENT_CLASS;
    root.style.cssText =
      "height:100%;width:100%;display:flex;flex-direction:column;overflow:hidden;";
    popoutDocument.body.appendChild(root);
  }
  return root;
}

function ensureBaseChrome(popoutDocument) {
  if (popoutDocument.getElementById("flex-popout-base")) return;
  const base = popoutDocument.createElement("style");
  base.id = "flex-popout-base";
  base.textContent = [
    "@font-face{font-family:'ChicagoFLF';src:url('" +
      new URL("/fonts/ChicagoFLF.ttf", window.location.origin).href +
      "') format('truetype');font-weight:normal;font-style:normal;}",
    "html,body{margin:0;padding:0;height:100%;width:100%;overflow:hidden;background:#CDCDCD;",
    "font-family:'ChicagoFLF','Geneva','Chicago',monospace;font-size:12px;color:#000;}",
    ":root{--color-gray:#CDCDCD;--color-dark-gray:#999;--color-black:#000;--color-white:#FFF;",
    "--color-primary:#8181FF;--bg-window:var(--color-gray);--bg-titlebar:var(--color-gray);",
    "--bg-input:var(--color-white);--bg-slider-track:var(--color-dark-gray);",
    "--border-color:var(--color-black);--text-color:var(--color-black);",
    "--border-width:1.5px;--border:var(--border-width) solid var(--border-color);",
    "--bevel-size:var(--border-width);",
    "--font-family:'ChicagoFLF','Geneva','Chicago',monospace;--radius:0px;}",
    "." + CONTENT_CLASS + "{height:100%;width:100%;}",
  ].join("");
  popoutDocument.head.appendChild(base);
}

function retainWindow(id, win) {
  let entry = windowRegistry.get(id);
  if (entry && entry.closeTimer) {
    clearTimeout(entry.closeTimer);
    entry.closeTimer = null;
  }
  if (entry && entry.win === win) {
    entry.owners += 1;
    return entry;
  }
  entry = { win: win, owners: 1, closeTimer: null };
  windowRegistry.set(id, entry);
  return entry;
}

function releaseWindow(id) {
  const entry = windowRegistry.get(id);
  if (!entry) return;
  entry.owners = Math.max(0, entry.owners - 1);
  if (entry.owners > 0) return;

  // Grace period: remount in the same tick cancels the close.
  if (entry.closeTimer) clearTimeout(entry.closeTimer);
  entry.closeTimer = setTimeout(() => {
    const cur = windowRegistry.get(id);
    if (!cur || cur.owners > 0) return;
    try {
      if (cur.win && !cur.win.closed) cur.win.close();
    } catch (_e) {
      /* ignore */
    }
    windowRegistry.delete(id);
  }, 80);
}

const FloatingWindow = (props) => {
  const { title, id, url, rect, onCloseWindow, onSetWindow, children } = props;
  const popoutWindow = React.useRef(null);
  const closedPollRef = React.useRef(null);
  const [content, setContent] = React.useState(undefined);

  React.useLayoutEffect(() => {
    let isMounted = true;
    const r = rect || new Rect_1.Rect(0, 0, 100, 100);

    const attachTo = (win) => {
      popoutWindow.current = win;
      retainWindow(id, win);
      onSetWindow(id, win);

      const mountRoot = () => {
        if (!isMounted || !popoutWindow.current || popoutWindow.current.closed) return;
        const doc = popoutWindow.current.document;
        ensureBaseChrome(doc);
        copyDomStyles(doc);
        setContent(ensureContentRoot(doc, title));
      };

      if (win.document.readyState === "complete") {
        mountRoot();
      } else {
        win.addEventListener("load", mountRoot, { once: true });
      }

      const onVisible = () => {
        if (!isMounted || !popoutWindow.current || popoutWindow.current.closed) return;
        const doc = popoutWindow.current.document;
        if (!doc || !doc.body) return;
        ensureBaseChrome(doc);
        copyDomStyles(doc);
        const root = ensureContentRoot(doc, title);
        setContent((prev) => (prev && doc.contains(prev) ? prev : root));
      };
      win.addEventListener("focus", onVisible);
      win.addEventListener("pageshow", onVisible);
      try {
        win.document.addEventListener("visibilitychange", onVisible);
      } catch (_e) {
        /* ignore */
      }

      if (closedPollRef.current) clearInterval(closedPollRef.current);
      closedPollRef.current = setInterval(() => {
        if (popoutWindow.current && popoutWindow.current.closed) {
          if (closedPollRef.current) clearInterval(closedPollRef.current);
          closedPollRef.current = null;
          windowRegistry.delete(id);
          onCloseWindow(id);
        }
      }, 400);

      return () => {
        win.removeEventListener("focus", onVisible);
        win.removeEventListener("pageshow", onVisible);
        try {
          win.document.removeEventListener("visibilitychange", onVisible);
        } catch (_e) {
          /* ignore */
        }
      };
    };

    let detachListeners = () => {};
    const existing = windowRegistry.get(id);
    if (existing && existing.win && !existing.win.closed) {
      detachListeners = attachTo(existing.win) || (() => {});
    } else {
      const win = window.open(
        url,
        id,
        "left=" + r.x + ",top=" + r.y + ",width=" + r.width + ",height=" + r.height,
      );
      if (win !== null) {
        detachListeners = attachTo(win) || (() => {});
      } else {
        console.warn("Unable to open window " + url);
        onCloseWindow(id);
      }
    }

    const onMainUnload = () => {
      const entry = windowRegistry.get(id);
      if (entry && entry.win && !entry.win.closed) entry.win.close();
      windowRegistry.delete(id);
    };
    window.addEventListener("beforeunload", onMainUnload);

    return () => {
      isMounted = false;
      detachListeners();
      window.removeEventListener("beforeunload", onMainUnload);
      if (closedPollRef.current) {
        clearInterval(closedPollRef.current);
        closedPollRef.current = null;
      }
      // Drop ownership — closes OS window unless another mount retains it.
      releaseWindow(id);
    };
  }, [id]);

  if (content !== undefined && content.isConnected) {
    return (0, react_dom_1.createPortal)(children, content);
  }
  return null;
};

exports.FloatingWindow = FloatingWindow;
