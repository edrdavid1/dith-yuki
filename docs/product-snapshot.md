# Dither Yuki 2.0.6-beta — product snapshot

Updated **2026-11-17**. The app version is `src-tauri/tauri.conf.json`.
This page is what is real in the current beta. Mechanism lives in the as-built
docs linked from [docs/README.md](./README.md).

| Documents | Ceiling | RAM cache | IPC |
|---|---|---|---|
| N tabs (runtime `DocumentId`) | 8192² | adaptive 25% of RAM, clamp 512 MiB–4 GiB (`DITHER_RAM_BUDGET_MIB`) | `commands/` + `services/` |

**Verdict.** A dither and palette studio: tabs, FlexLayout panels, ASCII (CPU, full document), Path B GPU on a warm viewport, OS previews for `.dyproj` / `.dyuki`, in-app updates, General / Gamedev workspace, display soft proof, and CMYK TIFF print export. It is not a paint app, not a video tool, and not a PDF/X press pipeline. Exact error diffusion stays on the CPU. Cold GPU compute is not on by default. GPU export is a no-go (A5).

## In the build

| Area | Where to read |
|---|---|
| Tabs, shared cache, T1–T6 pressure / JobBusy / memory-warning | [multi-doc-tabs.md](./multi-doc-tabs.md) |
| FlexLayout panels | [FLEXLAYOUT_DOCKING.md](./FLEXLAYOUT_DOCKING.md) |
| Path B GPU, auto-dispatch | [gpu-as-built.md](./gpu-as-built.md) |
| Exact error diffusion | CPU wavefront. Far from the origin it stays sequential. |
| Approximate GPU dither (`blocked_fs`, `blue_noise`) | On by default since 2.0.3-beta. Not bit-identical to Floyd–Steinberg. [adr/gpu-dither-families.md](./adr/gpu-dither-families.md) |
| ASCII | CPU only. [ascii-as-built.md](./ascii-as-built.md) |
| Soft proof | Display-only ICC simulation. User-imported profiles. No bundled FOGRA/PSO. [soft-proof-cmyk.md](./soft-proof-cmyk.md) |
| Print export | CMYK TIFF via lcms2. Not PDF/X, not plate screens. [print-export-cmyk.md](./print-export-cmyk.md) |
| Batch export | File → Batch Export. [gamedev-toolkit.md](./gamedev-toolkit.md) |
| Post above a dither barrier | On by default (`DITHER_POST_CACHE=0` turns it off). [post-barrier-roadmap.md](./post-barrier-roadmap.md) |
| Workspace | General (default) hides tiling UI; Gamedev shows it. [gamedev-toolkit.md](./gamedev-toolkit.md) |
| Crash journal | `.state` + `.blobs`. [crash-recovery.md](./crash-recovery.md) |
| Startup splash | `splash` window until `boot_finished`, 15 s watchdog. `src-tauri/src/splash.rs` |

## Still beta

| Limit | Why it matters |
|---|---|
| f32 tiles, 2 px halo | A viewport holds a lot of RAM per layer. f16 (A8) has not started. |
| Exact ED is sequential | Correct, and it fills from the document origin. |
| Cold GPU | About 3× slower than CPU on first touch. Cold compute is `DITHER_GPU_PREVIEW=1`, not the default. |
| No paint, no video, no PDF/X | Document model is structure plus undo snapshots. |
| ASCII stays on CPU | No default GPU ASCII path. |
| GPU export | A5 is a no-go. CPU is export truth. |
| Post on GPU after ED | Measured and deferred. [post-lever-measurement.md](./post-lever-measurement.md) |
| Large Glow / blur | GPU only while the radius fits the tile halo. Larger radii stay on CPU. |

## GPU, said plainly

Do not describe GPU as always faster. Warm pattern and grade stacks win. Cold pan loses. Any stack with exact error diffusion hits a CPU checkpoint. `blocked_fs` and `blue_noise` are a different, approximate path.

Env: `DITHER_GPU_PREVIEW=1` (cold compute), `DITHER_FORCE_CPU=1`, `DITHER_GPU_WARMUP=0`. There is no Preferences toggle.
