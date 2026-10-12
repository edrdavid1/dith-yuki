# Preview latency diagnostics

Instrumentation only. It does not change dithering, tile size, IPC, or the canvas.

## Enable

In `{app_data}/config.json`:

```json
"experimental": {
  "preview_diagnostics": {
    "enabled": true,
    "level": 1,
    "save_session_reports": false
  }
}
```

Restart the app. `enabled` defaults to false, including when the key is absent. Level 1 logs one `[diag/L1]` line per gesture to the dev console (frontend) and the Rust log target `preview_diag` (worker summary). Level 2 is reserved and not implemented yet.

## Read a frontend line

`debounce_ms` is the slider or viewport coalesce before `update_filter` / `set_viewport`. `param_ipc_ms` is that invoke. `backend_wait_ms` and `spread_ms` still mix queue, compute, `tile://`, HTTP 202 retries, and decode — `http_202` says how many retry sleeps landed in there. `l0_tasks` is on the following Rust line, joined by document id and generation, not by subtracting clocks.

`closed_by` is `all_visible` or `commit_timeout` (the existing 100 ms commit wait).

## Scenarios

Pin the canvas at 1920×1080. `z1.00_1920x1080` is zoom 1. A fit zoom is whatever the canvas used to fit the document. The slider move for the Bayer check is `threshold_scale` 1.0 → 1.2 on a 4096×4096 document, once at zoom 1 and once fit to the view.
