# dither_profile harness summary

backend env: `gpu`  device_ok=true warmed=true quick=true

## blocked_fs 748x1024 levels=16 path=GPU

| stage | median_ms | p95_ms |
|---|---:|---:|
| wall_apply | 218.14 | 235.75 |
| prepare | 0.00 | 0.00 |
| upload | 1.36 | 2.21 |
| dispatch_wall | 206.58 | 225.75 |
| readback | 1.89 | 2.70 |
| host_f32_to_u8 | 1.12 | 1.45 |
| host_u8_to_f32 | 0.70 | 2.38 |

## blocked_fs 748x1024 levels=256 path=GPU

| stage | median_ms | p95_ms |
|---|---:|---:|
| wall_apply | 221.55 | 238.25 |
| prepare | 0.00 | 0.00 |
| upload | 1.55 | 2.33 |
| dispatch_wall | 212.53 | 230.69 |
| readback | 1.92 | 3.06 |
| host_f32_to_u8 | 1.07 | 1.09 |
| host_u8_to_f32 | 0.75 | 1.01 |

## blocked_fs 1920x1080 levels=16 path=GPU

| stage | median_ms | p95_ms |
|---|---:|---:|
| wall_apply | 528.05 | 547.07 |
| prepare | 0.00 | 0.00 |
| upload | 3.93 | 4.06 |
| dispatch_wall | 512.21 | 530.29 |
| readback | 2.81 | 4.12 |
| host_f32_to_u8 | 1.55 | 2.12 |
| host_u8_to_f32 | 1.57 | 1.73 |

## blocked_fs 1920x1080 levels=256 path=GPU

| stage | median_ms | p95_ms |
|---|---:|---:|
| wall_apply | 516.62 | 546.12 |
| prepare | 0.00 | 0.00 |
| upload | 3.72 | 4.55 |
| dispatch_wall | 501.72 | 531.13 |
| readback | 2.75 | 3.85 |
| host_f32_to_u8 | 2.09 | 2.11 |
| host_u8_to_f32 | 1.50 | 2.13 |

## blue_noise 748x1024 levels=16 path=GPU

| stage | median_ms | p95_ms |
|---|---:|---:|
| wall_apply | 176.66 | 187.55 |
| prepare | 0.00 | 0.00 |
| upload | 0.61 | 1.49 |
| dispatch_wall | 143.07 | 155.28 |
| readback | 1.72 | 5.03 |
| host_f32_to_u8 | 0.72 | 1.16 |
| host_u8_to_f32 | 0.45 | 1.68 |

## blue_noise 748x1024 levels=256 path=GPU

| stage | median_ms | p95_ms |
|---|---:|---:|
| wall_apply | 151.48 | 185.44 |
| prepare | 0.00 | 0.00 |
| upload | 0.62 | 1.05 |
| dispatch_wall | 121.00 | 122.37 |
| readback | 1.60 | 1.64 |
| host_f32_to_u8 | 0.53 | 4.56 |
| host_u8_to_f32 | 0.46 | 1.60 |

## blue_noise 1920x1080 levels=16 path=GPU

| stage | median_ms | p95_ms |
|---|---:|---:|
| wall_apply | 339.46 | 367.23 |
| prepare | 0.00 | 0.00 |
| upload | 1.25 | 1.39 |
| dispatch_wall | 308.11 | 310.08 |
| readback | 2.01 | 4.29 |
| host_f32_to_u8 | 1.23 | 1.42 |
| host_u8_to_f32 | 1.01 | 6.83 |

## blue_noise 1920x1080 levels=256 path=GPU

| stage | median_ms | p95_ms |
|---|---:|---:|
| wall_apply | 337.95 | 340.17 |
| prepare | 0.00 | 0.00 |
| upload | 1.27 | 1.44 |
| dispatch_wall | 307.83 | 308.78 |
| readback | 1.99 | 4.54 |
| host_f32_to_u8 | 0.96 | 1.25 |
| host_u8_to_f32 | 0.93 | 1.18 |

## Micro: blocked_fs palette/levels 16 vs 256 @ 1920x1080

- levels=16: median=520.42ms p95=526.07ms
- levels=256: median=525.22ms p95=531.41ms

## Micro: host f32↔u8 @ 4000x3000 (inside apply status)

- in_f32_to_u8=6.38ms out_u8_to_f32=5.37ms path=GPU

## Notes

- `gpu_dispatch_done` GPU timestamp-query: **not measured** (device created without `TIMESTAMP_QUERY`).
- `dispatch_wall` includes GPU wait intertwined with submit/poll (wall clock).
- Full T0–T23 UI chain requires release app with `DITHER_PROFILE=1` and slider gestures; marks land in `profiling/results/<date>/marks.jsonl`.
