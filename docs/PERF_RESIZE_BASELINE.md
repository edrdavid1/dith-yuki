# Windows resize baseline (Phase 0-lite)

Diagnostic only. No fix is chosen here.

**Status: not run.** This session has no Windows machine, so there is no Performance trace, no WebView2 version, and no (a)/(b)/(c) decision. Do not treat a VM without GPU acceleration as evidence.

Remote debugging is already wired. On the Windows build, set `DITHER_REMOTE_DEVTOOLS=1` before launch. `webview_debug.rs` passes `--remote-debugging-port=9222`. From a Mac, open `chrome://inspect` or `http://<windows-ip>:9222`. DevTools changes absolute times; compare runs only against each other, with the inspector attached the same way every time.

Record for every run:

- Machine: real hardware or VM (VM without GPU does not count)
- WebView2 runtime version
- DPI scale: 100 / 125 / 150 / 200%
- Windows setting “Show window contents while dragging”
- Build: release

Resize by hand for 5–10 seconds. Capture a Performance trace.

| Experiment | Procedure | Result | Conclusion |
|---|---|---|---|
| **E1.** Empty page in the same build and window | One full-window colored `div`, no React app, no tiles. A temporary blue page was used for the macOS gap check and then removed; it is not in the build | not run | Lower bound of WebView2 itself |
| **E2.** Preview frozen | Welcome screen, no document open (no tile requests), then resize. A second run with a large document open | not run | Smooth only with a document → preview. Lag on the welcome screen → something else |
| **E3.** Performance / React Profiler | One resize gesture. Count commits. Note FlexLayout and React | not run | Layout/React share of the frame |

## Decision

Pending the owner, after one real-hardware run:

- **(a)** E1 also lags → WebView2 or that machine. Document it and change nothing.
- **(b)** E2 is smooth with no document and laggy with one → preview. A new small spec. Not this one.
- **(c)** E3 shows layout or React → only the conditional steps in the lite spec §5, after the owner confirms.

Fixes for (b) and (c) do not start from this file.

## macOS stripe check (owner)

Record the screen before and after the backdrop change: drag an edge, double-click the title bar, zoom. Main window, a flex popout, Preferences, Help. The gap should be `#CDCDCD`, not white. Startup should not flash white. This session did not record that.
