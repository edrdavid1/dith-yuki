# Dither profile (time-testing.md)

Instrumentation for `blocked_fs` / `blue_noise` release→pixels timing.

## Enable

```bash
export DITHER_PROFILE=1
# optional: DITHER_GPU_PATH=gpu|cpu|auto
```

When set, Rust writes JSONL marks under `profiling/results/<date>/marks.jsonl`.  
Frontend probes `dither_profile_enabled` and emits T0–T2 / T18–T23 via IPC.

## Headless apply grid

```bash
DITHER_PROFILE=1 DITHER_GPU_PATH=gpu DITHER_PROFILE_QUICK=1 \
  cargo test -p dither --release dither_profile_harness_main_grid \
  -- --ignored --nocapture --test-threads=1
```

Omit `DITHER_PROFILE_QUICK` for the full size×15-sample matrix (slow on CPU).

## Marks

See `.local-doc/time-testing.md`. T12 GPU timestamp-query is currently **not measured** (feature not requested on the device); wall `dispatch_ms` is recorded in mark extras instead.
